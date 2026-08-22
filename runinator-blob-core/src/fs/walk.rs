//! the listing walk.
//!
//! keys come from the `meta/` tree rather than `data/`, because a metadata filename is exactly the
//! key plus `.json` and yields the key with no stat call. only the page that is actually returned
//! has its descriptors read, so a bucket with a hundred thousand objects costs one directory walk
//! and at most `max-keys` reads.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::errors::BlobError;
use crate::listing::{ListRequest, ListResponse, ObjectSummary};
use crate::meta::ObjectMeta;

use super::paths::BucketPaths;

/// collect every key in the bucket, sorted. sorting is what makes the continuation token a plain
/// "resume after this key" cursor rather than server-side state.
pub(super) fn collect_keys(paths: &BucketPaths) -> Result<Vec<String>, BlobError> {
    let root = paths.meta_root();
    let mut keys = Vec::new();
    walk(&root, &root, &mut keys)?;
    keys.sort();
    Ok(keys)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), BlobError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(BlobError::Io(format!("listing {}: {err}", dir.display()))),
    };
    for entry in entries {
        let entry =
            entry.map_err(|err| BlobError::Io(format!("listing {}: {err}", dir.display())))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| BlobError::Io(format!("stat {}: {err}", path.display())))?;
        if file_type.is_dir() {
            walk(root, &path, out)?;
            continue;
        }
        if let Some(key) = key_from_meta_path(root, &path) {
            out.push(key);
        }
    }
    Ok(())
}

/// turn `<meta root>/a/b.json` back into the key `a/b`.
fn key_from_meta_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let text = relative.to_str()?;
    let key = text.strip_suffix(".json")?;
    // the walk only ever descends the meta tree, whose separators are the ones we wrote.
    Some(key.replace(std::path::MAIN_SEPARATOR, "/"))
}

/// apply prefix, delimiter, and paging to a sorted key list, then load the page's descriptors.
pub(super) fn page(
    paths: &BucketPaths,
    keys: &[String],
    request: &ListRequest,
) -> Result<ListResponse, BlobError> {
    let prefix = request.prefix.as_deref().unwrap_or("");
    let limit = request.effective_max_keys();
    let after = request.continuation_token.as_deref();

    let mut objects = Vec::new();
    let mut common_prefixes: BTreeSet<String> = BTreeSet::new();
    let mut truncated = false;
    let mut last_seen: Option<String> = None;

    for key in keys {
        if !key.starts_with(prefix) {
            continue;
        }
        if after.is_some_and(|token| key.as_str() <= token) {
            continue;
        }
        // A rolled-up prefix uses one page slot, matching S3.
        if let Some(rolled) = roll_up(key, prefix, request.delimiter.as_deref()) {
            if common_prefixes.contains(&rolled) {
                last_seen = Some(key.clone());
                continue;
            }
            if objects.len() + common_prefixes.len() >= limit {
                truncated = true;
                break;
            }
            common_prefixes.insert(rolled);
            last_seen = Some(key.clone());
            continue;
        }
        if objects.len() + common_prefixes.len() >= limit {
            truncated = true;
            break;
        }
        objects.push(summary(paths, key)?);
        last_seen = Some(key.clone());
    }

    Ok(ListResponse {
        objects,
        common_prefixes: common_prefixes.into_iter().collect(),
        is_truncated: truncated,
        next_continuation_token: truncated.then_some(last_seen).flatten(),
    })
}

/// the common prefix this key collapses into, or `None` when it should be listed outright.
fn roll_up(key: &str, prefix: &str, delimiter: Option<&str>) -> Option<String> {
    let delimiter = delimiter?;
    if delimiter.is_empty() {
        return None;
    }
    let rest = key.strip_prefix(prefix)?;
    let at = rest.find(delimiter)?;
    Some(format!("{prefix}{}{delimiter}", &rest[..at]))
}

fn summary(paths: &BucketPaths, key: &str) -> Result<ObjectSummary, BlobError> {
    let path: PathBuf = paths.meta_root().join(format!("{key}.json"));
    let bytes = std::fs::read(&path).map_err(|err| super::paths::read_error(&path, key, err))?;
    let meta: ObjectMeta = serde_json::from_slice(&bytes)
        .map_err(|err| BlobError::Io(format!("parsing {}: {err}", path.display())))?;
    Ok(ObjectSummary {
        key: meta.key,
        size: meta.size,
        sha256: meta.sha256,
        last_modified: meta.last_modified,
    })
}
