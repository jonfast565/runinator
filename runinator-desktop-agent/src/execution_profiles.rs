//! Approved, provider-agnostic profile collection and publication for the desktop agent.

use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use glob::Pattern;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::execution_profiles::{
    ExecutionProfile, ExecutionProfileCommand, ExecutionProfileHealth,
    ExecutionProfilePublishRequest, ExecutionProfileSource, ExecutionProfileStatusRequest,
    validate_bundle_path,
};
use sha2::{Digest, Sha256};

use crate::agent::{ConnectionState, SharedHandle, log_line};

const POLL_INTERVAL: Duration = Duration::from_secs(30);
const MANIFEST_PATH: &str = ".runinator-profile.json";
const MAX_ARCHIVE_BYTES: usize = 10 * 1024 * 1024;
const MAX_EXPANDED_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILES: usize = 1_000;

#[derive(Debug, Clone)]
pub struct LocalProfileStatus {
    pub id: uuid::Uuid,
    pub name: String,
    pub config_digest: String,
    pub approved: bool,
    pub message: String,
}

pub fn spawn(
    runtime: &tokio::runtime::Handle,
    client: AsyncApiClient<StaticLocator>,
    shared: SharedHandle,
    mut agent: tokio::sync::watch::Receiver<runinator_worker::AgentStatus>,
) {
    runtime.spawn(async move {
        loop {
            if agent.borrow().connection == ConnectionState::Stopped {
                return;
            }
            if let Err(error) = synchronize(&client, &shared).await {
                log_line(
                    &shared,
                    format!("Execution profile synchronization failed: {error}"),
                );
            }
            tokio::select! {
                changed = agent.changed() => {
                    if changed.is_err() || agent.borrow().connection == ConnectionState::Stopped {
                        return;
                    }
                    // A reconnect triggers an immediate definition/source refresh.
                }
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
            }
        }
    });
}

async fn synchronize(
    client: &AsyncApiClient<StaticLocator>,
    shared: &SharedHandle,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let profiles = client.list_execution_profiles().await?;
    let approvals = crate::config::load().approved_execution_profiles;
    let mut statuses = profiles
        .iter()
        .map(|profile| LocalProfileStatus {
            id: profile.id,
            name: profile.name.clone(),
            config_digest: profile.config_digest.clone(),
            approved: approvals.get(&profile.id) == Some(&profile.config_digest),
            message: if !profile.enabled {
                "disabled centrally".into()
            } else if approvals.get(&profile.id) == Some(&profile.config_digest) {
                "approved; checking sources".into()
            } else {
                "local approval required".into()
            },
        })
        .collect::<Vec<_>>();

    for (index, profile) in profiles.into_iter().enumerate() {
        if !profile.enabled || !statuses[index].approved {
            continue;
        }
        let previous_revision = profile.current_revision;
        let force_refresh = profile.refresh_requested_at.is_some_and(|requested| {
            profile
                .published_at
                .is_none_or(|published| requested > published)
        });
        let result = tokio::task::spawn_blocking(move || collect(&profile, force_refresh)).await?;
        match result {
            Ok((id, bytes, digest)) => {
                let request = ExecutionProfilePublishRequest {
                    digest,
                    expires_at: None,
                };
                match client.publish_execution_profile(id, &request, bytes).await {
                    Ok(revision) => {
                        statuses[index].message =
                            format!("published revision {}", revision.revision);
                        if previous_revision != Some(revision.revision) {
                            log_line(
                                shared,
                                format!(
                                    "Execution profile '{}' is available at revision {}.",
                                    statuses[index].name, revision.revision
                                ),
                            );
                        }
                    }
                    Err(error) => {
                        statuses[index].message = format!("publication failed: {error}");
                        let _ = client
                            .report_execution_profile_status(
                                statuses[index].id,
                                &ExecutionProfileStatusRequest {
                                    health: ExecutionProfileHealth::Error,
                                    error: Some(
                                        "desktop publication failed; inspect the desktop agent log"
                                            .into(),
                                    ),
                                },
                            )
                            .await;
                    }
                }
            }
            Err(error) => {
                statuses[index].message = format!("collection failed: {error}");
                log_line(
                    shared,
                    format!(
                        "Execution profile '{}' collection failed: {error}",
                        statuses[index].name
                    ),
                );
                let _ = client
                    .report_execution_profile_status(
                        statuses[index].id,
                        &ExecutionProfileStatusRequest {
                            health: ExecutionProfileHealth::Error,
                            error: Some(
                                "desktop collection failed; inspect the desktop agent log".into(),
                            ),
                        },
                    )
                    .await;
            }
        }
    }
    shared
        .lock()
        .expect("desktop agent state lock poisoned")
        .execution_profiles = statuses;
    Ok(())
}

fn collect(
    profile: &ExecutionProfile,
    force_refresh: bool,
) -> Result<(uuid::Uuid, Vec<u8>, String), Box<dyn std::error::Error + Send + Sync>> {
    if force_refresh {
        let refresh = profile
            .collection
            .refresh
            .as_ref()
            .ok_or("a refresh was requested but no refresh command is configured")?;
        if !run_command(refresh, true)?.status.success() {
            return Err("profile refresh command failed".into());
        }
    }
    if let Some(probe) = &profile.collection.probe
        && !run_command(probe, false)?.status.success()
    {
        if force_refresh {
            return Err("profile probe still fails after requested refresh".into());
        }
        let refresh = profile
            .collection
            .refresh
            .as_ref()
            .ok_or("profile probe failed and no refresh command is configured")?;
        if !run_command(refresh, true)?.status.success() {
            return Err("profile refresh command failed".into());
        }
        if !run_command(probe, false)?.status.success() {
            return Err("profile probe still fails after refresh".into());
        }
    }

    let mut files = BTreeMap::<String, Vec<u8>>::new();
    for source in &profile.collection.sources {
        match source {
            ExecutionProfileSource::File { path, target } => {
                insert(&mut files, target, fs::read(expand_path(path))?)?;
            }
            ExecutionProfileSource::Directory { path, glob, target } => {
                collect_directory(&mut files, &expand_path(path), target, &Pattern::new(glob)?)?;
            }
            ExecutionProfileSource::Command { command, target } => {
                if command.interactive {
                    return Err(
                        "a command source cannot be interactive because its stdout becomes a file"
                            .into(),
                    );
                }
                let output = run_command(command, false)?;
                if !output.status.success() {
                    return Err(format!("command source exited with {}", output.status).into());
                }
                insert(&mut files, target, output.stdout)?;
            }
        }
    }

    let manifest = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "profile_id": profile.id,
        "config_digest": profile.config_digest,
        "files": files.iter().map(|(path, bytes)| serde_json::json!({
            "path": path,
            "sha256": format!("{:x}", Sha256::digest(bytes)),
            "size": bytes.len(),
        })).collect::<Vec<_>>()
    }))?;
    if files.len() + 1 > MAX_FILES
        || files.values().map(Vec::len).sum::<usize>() > MAX_EXPANDED_BYTES
    {
        return Err("execution profile exceeds the file-count or expanded-size limit".into());
    }
    files.insert(MANIFEST_PATH.into(), manifest);

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (path, bytes) in files {
        writer.start_file(path, options)?;
        writer.write_all(&bytes)?;
    }
    let bytes = writer.finish()?.into_inner();
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err("execution profile archive exceeds 10 MiB".into());
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok((profile.id, bytes, digest))
}

fn insert(
    files: &mut BTreeMap<String, Vec<u8>>,
    target: &str,
    bytes: Vec<u8>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    validate_bundle_path(target).map_err(|error| format!("invalid target '{target}': {error}"))?;
    if target == MANIFEST_PATH || files.insert(target.into(), bytes).is_some() {
        return Err(format!("duplicate or reserved profile target '{target}'").into());
    }
    Ok(())
}

fn collect_directory(
    files: &mut BTreeMap<String, Vec<u8>>,
    root: &Path,
    target: &str,
    pattern: &Pattern,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    fn walk(
        files: &mut BTreeMap<String, Vec<u8>>,
        root: &Path,
        current: &Path,
        target: &str,
        pattern: &Pattern,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let metadata = entry.file_type()?;
            if metadata.is_symlink() {
                return Err(
                    format!("profile source contains link: {}", entry.path().display()).into(),
                );
            }
            if metadata.is_dir() {
                walk(files, root, &entry.path(), target, pattern)?;
            } else if metadata.is_file() {
                let relative = entry.path().strip_prefix(root)?.to_path_buf();
                let relative_text = relative.to_string_lossy().replace('\\', "/");
                if pattern.matches(&relative_text) || pattern.matches_path(&relative) {
                    let mapped = format!("{}/{}", target.trim_end_matches('/'), relative_text)
                        .trim_start_matches('/')
                        .to_string();
                    insert(files, &mapped, fs::read(entry.path())?)?;
                }
            }
        }
        Ok(())
    }
    validate_bundle_path(target).map_err(|error| format!("invalid directory target: {error}"))?;
    walk(files, root, root, target, pattern)
}

fn run_command(
    command: &ExecutionProfileCommand,
    permit_interactive: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    let (program, args) = command.argv.split_first().ok_or("command argv is empty")?;
    if command.interactive && !permit_interactive {
        return Err("interactive commands are allowed only for refresh".into());
    }
    let mut child = Command::new(program);
    child.args(args).stdin(Stdio::null());
    if command.interactive {
        child.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        let status = child.status()?;
        Ok(std::process::Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    } else {
        Ok(child.output()?)
    }
}

fn expand_path(raw: &str) -> PathBuf {
    if (raw == "~" || raw.starts_with("~/"))
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(raw.trim_start_matches("~/"));
    }
    let path = PathBuf::from(raw);
    // Collection paths may be absolute on the approved desktop. Normalizing `.` here avoids
    // accidental duplicate spellings while preserving the user's explicitly approved location.
    path.components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use runinator_models::execution_profiles::{
        ExecutionProfileCollectionSpec, ExecutionProfileExposureSpec, ExecutionProfileHealth,
    };

    #[test]
    fn collection_maps_files_and_directories_into_one_deterministic_archive() {
        let root =
            std::env::temp_dir().join(format!("runinator-profile-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("cache/nested")).unwrap();
        fs::write(root.join("config"), b"profile config").unwrap();
        fs::write(root.join("cache/token.json"), b"token").unwrap();
        fs::write(root.join("cache/nested/ignored.txt"), b"ignored").unwrap();
        let profile = ExecutionProfile {
            id: uuid::Uuid::new_v4(),
            org_id: None,
            name: "fixture".into(),
            description: String::new(),
            credential_scopes: vec!["fixture".into()],
            collection: ExecutionProfileCollectionSpec {
                version: 1,
                probe: None,
                refresh: None,
                sources: vec![
                    ExecutionProfileSource::File {
                        path: root.join("config").to_string_lossy().into_owned(),
                        target: ".tool/config".into(),
                    },
                    ExecutionProfileSource::Directory {
                        path: root.join("cache").to_string_lossy().into_owned(),
                        glob: "*.json".into(),
                        target: ".tool/cache".into(),
                    },
                ],
            },
            exposure: ExecutionProfileExposureSpec::default(),
            config_version: 1,
            config_digest: "config-digest".into(),
            enabled: true,
            current_revision: None,
            current_digest: None,
            current_publisher_id: None,
            published_at: None,
            expires_at: None,
            refresh_requested_at: None,
            health: ExecutionProfileHealth::Unpublished,
            last_error: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let (_, first, first_digest) = collect(&profile, false).unwrap();
        let (_, second, second_digest) = collect(&profile, false).unwrap();
        assert_eq!(first, second);
        assert_eq!(first_digest, second_digest);
        let archive = zip::ZipArchive::new(Cursor::new(first)).unwrap();
        let names = archive.file_names().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                ".runinator-profile.json",
                ".tool/cache/token.json",
                ".tool/config"
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }
}
