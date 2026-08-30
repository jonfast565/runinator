//! retention for timestamped release images pushed through `--local-registry`.
//!
//! Docker Distribution deletes manifests by digest, not individual tags. Before deleting an old
//! release, the pruner therefore resolves every tag and protects a digest if any retained or
//! operator-created tag still points at it.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Url;
use reqwest::blocking::Client;
use reqwest::header::ACCEPT;

const MANIFEST_ACCEPT: &str = "application/vnd.oci.image.manifest.v1+json, \
application/vnd.oci.image.index.v1+json, \
application/vnd.docker.distribution.manifest.v2+json, \
application/vnd.docker.distribution.manifest.list.v2+json";
const CONTENT_DIGEST: &str = "docker-content-digest";

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd)]
struct ReleaseKey {
    timestamp: u64,
    major: u64,
    minor: u64,
    build: u64,
    tag: String,
}

#[derive(Debug)]
struct RegistryLocation {
    api_base: Url,
    repository_prefix: String,
}

impl RegistryLocation {
    fn parse(value: &str) -> Result<Self> {
        let value = value.trim().trim_end_matches('/');
        anyhow::ensure!(!value.is_empty(), "local registry cannot be empty");
        anyhow::ensure!(
            !value.contains("://"),
            "--local-registry must be a Docker registry reference without a URL scheme"
        );

        let (authority, repository_prefix) = value
            .split_once('/')
            .map_or((value, ""), |(authority, prefix)| (authority, prefix));
        anyhow::ensure!(!authority.is_empty(), "local registry host cannot be empty");
        let api_base = Url::parse(&format!("http://{authority}/"))
            .with_context(|| format!("invalid local registry '{value}'"))?;
        anyhow::ensure!(
            api_base.host_str().is_some(),
            "local registry '{value}' has no host"
        );

        Ok(Self {
            api_base,
            repository_prefix: repository_prefix.trim_matches('/').to_string(),
        })
    }

    fn repository(&self, image_name: &str) -> String {
        if self.repository_prefix.is_empty() {
            image_name.to_string()
        } else {
            format!("{}/{image_name}", self.repository_prefix)
        }
    }

    fn endpoint(&self, repository: &str, operation: &str, reference: Option<&str>) -> Url {
        let mut url = self.api_base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .expect("an HTTP registry URL supports path segments");
            segments.push("v2");
            for segment in repository.split('/') {
                segments.push(segment);
            }
            segments.push(operation);
            if let Some(reference) = reference {
                segments.push(reference);
            }
        }
        url
    }
}

/// Removes timestamped Runinator manifests beyond `retention` for every image pushed by this
/// deployment. Explicit/custom tags and any digest they share remain untouched.
pub fn prune_release_images<'a>(
    local_registry: &str,
    image_names: impl IntoIterator<Item = &'a str>,
    retention: usize,
) -> Result<()> {
    if retention == 0 {
        return Ok(());
    }

    let location = RegistryLocation::parse(local_registry)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to create local-registry client")?;

    for image_name in image_names {
        prune_repository(
            &client,
            &location,
            &location.repository(image_name),
            retention,
        )?;
    }
    Ok(())
}

fn prune_repository(
    client: &Client,
    location: &RegistryLocation,
    repository: &str,
    retention: usize,
) -> Result<()> {
    let tags_url = location.endpoint(repository, "tags", Some("list"));
    let response = client
        .get(tags_url.clone())
        .send()
        .with_context(|| format!("failed to list tags for {repository} at {tags_url}"))?
        .error_for_status()
        .with_context(|| format!("local registry rejected tag listing for {repository}"))?;
    let body: serde_json::Value = response
        .json()
        .with_context(|| format!("local registry returned invalid tag JSON for {repository}"))?;
    let tags: Vec<String> = body
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let delete_tags = tags_to_prune(&tags, retention);
    if delete_tags.is_empty() {
        println!(
            "    {repository}: {} release tag(s), nothing to prune",
            release_tag_count(&tags)
        );
        return Ok(());
    }

    // Manifests are deleted by digest. Resolve all tags so a custom alias or retained release tag
    // can protect a shared manifest from being removed accidentally.
    let mut digest_tags: HashMap<String, Vec<String>> = HashMap::new();
    let mut tag_digests = HashMap::new();
    for tag in &tags {
        let digest = manifest_digest(client, location, repository, tag)?;
        digest_tags
            .entry(digest.clone())
            .or_default()
            .push(tag.clone());
        tag_digests.insert(tag.clone(), digest);
    }

    let mut deleted_manifests = 0usize;
    let mut protected_tags = 0usize;
    let mut visited = HashSet::new();
    for tag in &delete_tags {
        let digest = tag_digests
            .get(tag)
            .expect("the deletion candidate digest was resolved above")
            .clone();
        if !visited.insert(digest.clone()) {
            continue;
        }
        let aliases = digest_tags
            .get(&digest)
            .expect("the deletion candidate digest was resolved above");
        if aliases.iter().any(|alias| !delete_tags.contains(alias)) {
            protected_tags += aliases
                .iter()
                .filter(|alias| delete_tags.contains(*alias))
                .count();
            continue;
        }

        let delete_url = location.endpoint(repository, "manifests", Some(&digest));
        client
            .delete(delete_url)
            .send()
            .with_context(|| format!("failed to delete {repository}@{digest}"))?
            .error_for_status()
            .with_context(|| {
                format!(
                    "local registry rejected deletion of {repository}@{digest}; ensure manifest deletion is enabled"
                )
            })?;
        deleted_manifests += 1;
    }

    println!(
        "    {repository}: deleted {deleted_manifests} old manifest(s), retained newest {retention} release version(s){}",
        if protected_tags == 0 {
            String::new()
        } else {
            format!(", protected {protected_tags} tag(s) sharing another alias")
        }
    );
    Ok(())
}

fn manifest_digest(
    client: &Client,
    location: &RegistryLocation,
    repository: &str,
    reference: &str,
) -> Result<String> {
    let manifest_url = location.endpoint(repository, "manifests", Some(reference));
    let response = client
        .head(manifest_url)
        .header(ACCEPT, MANIFEST_ACCEPT)
        .send()
        .with_context(|| format!("failed to inspect {repository}:{reference}"))?
        .error_for_status()
        .with_context(|| {
            format!("local registry rejected manifest lookup for {repository}:{reference}")
        })?;
    response
        .headers()
        .get(CONTENT_DIGEST)
        .context("manifest response omitted Docker-Content-Digest")?
        .to_str()
        .context("manifest digest header was not valid text")
        .map(str::to_string)
}

fn release_key(tag: &str) -> Option<ReleaseKey> {
    let (version, timestamp) = match tag.rsplit_once("-kube-") {
        Some(parts) => parts,
        None => ("0.0.0", tag.strip_prefix("kube-")?),
    };
    if timestamp.len() != 14 || !timestamp.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut version_parts = version.split('.');
    let major = version_parts.next()?.parse().ok()?;
    let minor = version_parts.next()?.parse().ok()?;
    let build = version_parts.next()?.parse().ok()?;
    if version_parts.next().is_some() {
        return None;
    }
    Some(ReleaseKey {
        timestamp: timestamp.parse().ok()?,
        major,
        minor,
        build,
        tag: tag.to_string(),
    })
}

fn release_tag_count(tags: &[String]) -> usize {
    tags.iter().filter(|tag| release_key(tag).is_some()).count()
}

fn tags_to_prune(tags: &[String], retention: usize) -> HashSet<String> {
    let mut releases: Vec<ReleaseKey> = tags.iter().filter_map(|tag| release_key(tag)).collect();
    releases.sort_unstable_by(|left, right| right.cmp(left));
    releases
        .into_iter()
        .skip(retention)
        .map(|release| release.tag)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{RegistryLocation, release_key, tags_to_prune};
    use std::collections::HashSet;

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn release_key_accepts_only_timestamped_semver_deploy_tags() {
        assert!(release_key("0.5.542-kube-20260830010203").is_some());
        assert!(release_key("kube-20260830010203").is_some());
        assert!(release_key("latest").is_none());
        assert!(release_key("0.5.542").is_none());
        assert!(release_key("0.5-kube-20260830010203").is_none());
        assert!(release_key("0.5.542-kube-not-a-date").is_none());
    }

    #[test]
    fn pruning_keeps_the_newest_configured_number_and_ignores_custom_tags() {
        let input = tags(&[
            "latest",
            "kube-20260828000000",
            "0.5.9-kube-20260829000000",
            "0.5.10-kube-20260830000000",
            "0.5.11-kube-20260831000000",
            "manual",
        ]);
        assert_eq!(
            tags_to_prune(&input, 2),
            HashSet::from([
                "kube-20260828000000".to_string(),
                "0.5.9-kube-20260829000000".to_string()
            ])
        );
    }

    #[test]
    fn local_registry_path_becomes_a_repository_prefix() {
        let location = RegistryLocation::parse("localhost:5000/team/runinator/").unwrap();
        assert_eq!(location.api_base.as_str(), "http://localhost:5000/");
        assert_eq!(
            location.repository("runinator-ws"),
            "team/runinator/runinator-ws"
        );
        assert_eq!(
            location
                .endpoint("team/runinator/runinator-ws", "tags", Some("list"))
                .as_str(),
            "http://localhost:5000/v2/team/runinator/runinator-ws/tags/list"
        );
    }
}
