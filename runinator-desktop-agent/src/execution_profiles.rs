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
    ExecutionProfile, ExecutionProfileAgentStatusRequest, ExecutionProfileApprovalState,
    ExecutionProfileCommand, ExecutionProfileOperation, ExecutionProfileOperationClaimRequest,
    ExecutionProfileOperationCompleteRequest, ExecutionProfileOperationKind,
    ExecutionProfileOperationState, ExecutionProfilePublishRequest, ExecutionProfileSource,
    validate_bundle_path,
};
use sha2::{Digest, Sha256};

use crate::agent::{ConnectionState, SharedHandle, log_line};

pub const PROFILE_SYNC_INTERVAL: Duration = Duration::from_secs(30);
const MANIFEST_PATH: &str = ".runinator-profile.json";
const MAX_ARCHIVE_BYTES: usize = 10 * 1024 * 1024;
const MAX_EXPANDED_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILES: usize = 1_000;
const MAX_STATUS_ERROR_CHARS: usize = 512;

#[derive(Debug, Clone)]
pub struct LocalProfileStatus {
    pub id: uuid::Uuid,
    pub name: String,
    pub config_digest: String,
    pub enabled: bool,
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
            // collection may invoke an interactive desktop command (for example, a Keychain
            // access prompt). do not start it while the worker is still registering or connecting:
            // an approval must never make the agent appear to be stuck before its action loop is
            // actually available.
            if !wait_until_running(&mut agent).await {
                return;
            }
            match synchronize(&client, |message| log_line(&shared, message)).await {
                Ok(statuses) => {
                    shared
                        .lock()
                        .expect("desktop agent state lock poisoned")
                        .execution_profiles = statuses;
                }
                Err(error) => {
                    log_line(
                        &shared,
                        format!("Execution profile synchronization failed: {error}"),
                    );
                }
            }
            tokio::select! {
                changed = agent.changed() => {
                    if changed.is_err() || agent.borrow().connection == ConnectionState::Stopped {
                        return;
                    }
                    // A reconnect triggers an immediate definition/source refresh.
                }
                _ = tokio::time::sleep(PROFILE_SYNC_INTERVAL) => {}
            }
        }
    });
}

/// wait until the worker has registered and begun serving work before touching profile sources.
/// a stopped lifecycle or a dropped status stream cannot become usable without a new start.
pub(crate) async fn wait_until_running(
    agent: &mut tokio::sync::watch::Receiver<runinator_worker::AgentStatus>,
) -> bool {
    loop {
        let can_continue = {
            let status = agent.borrow();
            if collection_can_run(&status) {
                return true;
            }
            status.connection != ConnectionState::Stopped
        };
        if !can_continue {
            return false;
        }
        if agent.changed().await.is_err() {
            return false;
        }
    }
}

fn collection_can_run(status: &runinator_worker::AgentStatus) -> bool {
    status.running && status.connection.is_connected()
}

pub async fn synchronize(
    client: &AsyncApiClient<StaticLocator>,
    log: impl Fn(String),
) -> Result<Vec<LocalProfileStatus>, Box<dyn std::error::Error + Send + Sync>> {
    let profiles = client.list_execution_profiles().await?;
    let mut pending_operations = client
        .list_pending_execution_profile_operations()
        .await?
        .into_iter()
        .map(|operation| (operation.profile_id, operation))
        .collect::<BTreeMap<_, _>>();
    let approvals = crate::config::load().approved_execution_profiles;
    let mut statuses = profiles
        .iter()
        .map(|profile| LocalProfileStatus {
            id: profile.id,
            name: profile.name.clone(),
            config_digest: profile.config_digest.clone(),
            enabled: profile.enabled,
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
        let approved = statuses[index].approved;
        report_agent_status(client, &profile, approved, None, None, None).await;
        let pending_operation = pending_operations.remove(&profile.id);
        if !profile.enabled || !approved {
            if let Some(operation) = pending_operation {
                statuses[index].message = format!(
                    "{} awaiting local approval",
                    operation_label(operation.kind)
                );
            }
            continue;
        }
        let previous_revision = profile.current_revision;
        let operation = match pending_operation {
            Some(operation) => match client
                .claim_execution_profile_operation(
                    operation.id,
                    &ExecutionProfileOperationClaimRequest {
                        config_digest: profile.config_digest.clone(),
                    },
                )
                .await
            {
                Ok(operation) => Some(operation),
                Err(_) => {
                    statuses[index].message =
                        "collection operation claimed by another desktop".into();
                    continue;
                }
            },
            None => None,
        };
        let dry_run = operation
            .as_ref()
            .is_some_and(|operation| operation.kind == ExecutionProfileOperationKind::DryRun);
        let force_refresh = operation
            .as_ref()
            .is_some_and(|operation| operation.kind == ExecutionProfileOperationKind::Refresh)
            || profile.refresh_requested_at.is_some_and(|requested| {
                profile
                    .published_at
                    .is_none_or(|published| requested > published)
            });
        let collection_profile = profile.clone();
        let result = tokio::task::spawn_blocking(move || {
            collect(&collection_profile, force_refresh && !dry_run, dry_run)
        })
        .await?;
        match result {
            Ok((id, bytes, digest)) => {
                if dry_run {
                    statuses[index].message = "dry run passed; no revision published".into();
                    let completed_at = chrono::Utc::now();
                    report_agent_status(
                        client,
                        &profile,
                        true,
                        Some(completed_at),
                        Some(completed_at),
                        None,
                    )
                    .await;
                    complete_operation(client, operation.as_ref(), None).await;
                    continue;
                }
                let request = ExecutionProfilePublishRequest {
                    digest,
                    expires_at: None,
                };
                match client.publish_execution_profile(id, &request, bytes).await {
                    Ok(revision) => {
                        statuses[index].message =
                            format!("published revision {}", revision.revision);
                        if previous_revision != Some(revision.revision) {
                            log(format!(
                                "Execution profile '{}' is available at revision {}.",
                                statuses[index].name, revision.revision
                            ));
                        }
                        let completed_at = chrono::Utc::now();
                        report_agent_status(
                            client,
                            &profile,
                            true,
                            Some(completed_at),
                            Some(completed_at),
                            None,
                        )
                        .await;
                        complete_operation(client, operation.as_ref(), None).await;
                    }
                    Err(error) => {
                        statuses[index].message = format!("publication failed: {error}");
                        let detail = status_error("desktop publication failed", &error);
                        report_agent_status(
                            client,
                            &profile,
                            true,
                            Some(chrono::Utc::now()),
                            None,
                            Some(detail.clone()),
                        )
                        .await;
                        complete_operation(client, operation.as_ref(), Some(detail)).await;
                    }
                }
            }
            Err(error) => {
                let action = if dry_run {
                    "collection dry run"
                } else {
                    "collection"
                };
                statuses[index].message = format!("{action} failed: {error}");
                log(format!(
                    "Execution profile '{}' {action} failed: {error}",
                    statuses[index].name
                ));
                let detail = status_error(&format!("desktop {action} failed"), &error);
                report_agent_status(
                    client,
                    &profile,
                    true,
                    Some(chrono::Utc::now()),
                    None,
                    Some(detail.clone()),
                )
                .await;
                complete_operation(client, operation.as_ref(), Some(detail)).await;
            }
        }
    }
    Ok(statuses)
}

async fn report_agent_status(
    client: &AsyncApiClient<StaticLocator>,
    profile: &ExecutionProfile,
    approved: bool,
    last_attempt_at: Option<chrono::DateTime<chrono::Utc>>,
    last_success_at: Option<chrono::DateTime<chrono::Utc>>,
    last_error: Option<String>,
) {
    let _ = client
        .report_execution_profile_agent_status(
            profile.id,
            &ExecutionProfileAgentStatusRequest {
                config_digest: profile.config_digest.clone(),
                approval: if approved {
                    ExecutionProfileApprovalState::Approved
                } else {
                    ExecutionProfileApprovalState::ApprovalRequired
                },
                last_attempt_at,
                last_success_at,
                last_error,
            },
        )
        .await;
}

async fn complete_operation(
    client: &AsyncApiClient<StaticLocator>,
    operation: Option<&ExecutionProfileOperation>,
    error: Option<String>,
) {
    let Some(operation) = operation else {
        return;
    };
    let _ = client
        .complete_execution_profile_operation(
            operation.id,
            &ExecutionProfileOperationCompleteRequest {
                state: if error.is_some() {
                    ExecutionProfileOperationState::Failed
                } else {
                    ExecutionProfileOperationState::Succeeded
                },
                error,
            },
        )
        .await;
}

fn operation_label(kind: ExecutionProfileOperationKind) -> &'static str {
    match kind {
        ExecutionProfileOperationKind::DryRun => "dry run",
        ExecutionProfileOperationKind::Refresh => "refresh and publication",
    }
}

fn collect(
    profile: &ExecutionProfile,
    force_refresh: bool,
    dry_run: bool,
) -> Result<(uuid::Uuid, Vec<u8>, String), Box<dyn std::error::Error + Send + Sync>> {
    if force_refresh {
        let refresh = profile
            .collection
            .refresh
            .as_ref()
            .ok_or("a refresh was requested but no refresh command is configured")?;
        run_checked_command(refresh, "profile refresh", true)?;
    }
    if let Some(probe) = &profile.collection.probe
        && let Err(error) = run_checked_command(probe, "profile probe", false)
    {
        if dry_run {
            return Err(format!("{error} during dry run").into());
        }
        if force_refresh {
            return Err(format!("{error} after requested refresh").into());
        }
        let refresh = profile
            .collection
            .refresh
            .as_ref()
            .ok_or("profile probe failed and no refresh command is configured")?;
        run_checked_command(refresh, "profile refresh", true)?;
        if let Err(error) = run_checked_command(probe, "profile probe", false) {
            return Err(format!("{error} after refresh").into());
        }
    }

    let mut files = BTreeMap::<String, Vec<u8>>::new();
    for source in &profile.collection.sources {
        match source {
            ExecutionProfileSource::File { path, target } => {
                let expanded = expand_path(path);
                let bytes = fs::read(&expanded).map_err(|error| {
                    format!(
                        "cannot read profile file source '{}' for target '{}': {error}",
                        expanded.display(),
                        target
                    )
                })?;
                insert(&mut files, target, bytes)?;
            }
            ExecutionProfileSource::Directory { path, glob, target } => {
                let expanded = expand_path(path);
                let pattern = Pattern::new(glob).map_err(|error| {
                    format!(
                        "invalid glob '{}' for profile directory source '{}' targeting '{}': {error}",
                        glob,
                        expanded.display(),
                        target
                    )
                })?;
                collect_directory(&mut files, &expanded, target, &pattern)?;
            }
            ExecutionProfileSource::Command { command, target } => {
                if command.interactive {
                    return Err(
                        "a command source cannot be interactive because its stdout becomes a file"
                            .into(),
                    );
                }
                let output = run_checked_command(
                    command,
                    &format!("profile command source for target '{target}'"),
                    false,
                )?;
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
        let mut entries = fs::read_dir(current)
            .map_err(|error| {
                format!(
                    "cannot read profile directory source '{}' for target '{}': {error}",
                    current.display(),
                    target
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                format!(
                    "cannot enumerate profile directory source '{}' for target '{}': {error}",
                    current.display(),
                    target
                )
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let entry_path = entry.path();
            let metadata = entry.file_type().map_err(|error| {
                format!(
                    "cannot inspect profile directory entry '{}' for target '{}': {error}",
                    entry_path.display(),
                    target
                )
            })?;
            if metadata.is_symlink() {
                return Err(format!(
                    "profile directory source contains link: {}",
                    entry_path.display()
                )
                .into());
            }
            if metadata.is_dir() {
                walk(files, root, &entry_path, target, pattern)?;
            } else if metadata.is_file() {
                let relative = entry_path
                    .strip_prefix(root)
                    .map_err(|error| {
                        format!(
                            "cannot map profile directory entry '{}' below source '{}': {error}",
                            entry_path.display(),
                            root.display()
                        )
                    })?
                    .to_path_buf();
                let relative_text = relative.to_string_lossy().replace('\\', "/");
                if pattern.matches(&relative_text) || pattern.matches_path(&relative) {
                    let mapped = format!("{}/{}", target.trim_end_matches('/'), relative_text)
                        .trim_start_matches('/')
                        .to_string();
                    let bytes = fs::read(&entry_path).map_err(|error| {
                        format!(
                            "cannot read profile directory entry '{}' for target '{}': {error}",
                            entry_path.display(),
                            mapped
                        )
                    })?;
                    insert(files, &mapped, bytes)?;
                }
            }
        }
        Ok(())
    }
    validate_bundle_path(target).map_err(|error| format!("invalid directory target: {error}"))?;
    walk(files, root, root, target, pattern)
}

fn run_checked_command(
    command: &ExecutionProfileCommand,
    label: &str,
    permit_interactive: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    let output = run_command(command, permit_interactive).map_err(|error| {
        format!(
            "{label} command '{}' could not start: {error}",
            command_program(command)
        )
    })?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!(
            "{label} command '{}' exited with {}",
            command_program(command),
            output.status
        )
        .into())
    }
}

fn command_program(command: &ExecutionProfileCommand) -> &str {
    command
        .argv
        .first()
        .map(String::as_str)
        .unwrap_or("<empty>")
}

fn status_error(prefix: &str, error: &dyn std::fmt::Display) -> String {
    let detail = format!("{prefix}: {error}");
    if detail.chars().count() <= MAX_STATUS_ERROR_CHARS {
        return detail;
    }
    let mut shortened = detail
        .chars()
        .take(MAX_STATUS_ERROR_CHARS.saturating_sub(1))
        .collect::<String>();
    shortened.push('…');
    shortened
}

fn run_command(
    command: &ExecutionProfileCommand,
    permit_interactive: bool,
) -> Result<std::process::Output, Box<dyn std::error::Error + Send + Sync>> {
    let (program, args) = command.argv.split_first().ok_or("command argv is empty")?;
    if command.interactive && !permit_interactive {
        return Err("interactive commands are allowed only for refresh".into());
    }
    let mut child = Command::new(resolve_command_program(program));
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

/// locate the bundled macOS Keychain collector when a profile uses its portable command name.
/// other command sources retain normal `PATH` lookup (or their explicitly configured path).
fn resolve_command_program(program: &str) -> PathBuf {
    if program != "keychain-export" {
        return PathBuf::from(program);
    }
    keychain_export_path().unwrap_or_else(|| PathBuf::from(program))
}

fn keychain_export_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    bundled_keychain_export(&executable)
        .or_else(cargo_built_keychain_export)
        .or_else(|| development_keychain_export(&executable))
}

fn cargo_built_keychain_export() -> Option<PathBuf> {
    let path = PathBuf::from(option_env!("RUNINATOR_KEYCHAIN_EXPORT_PATH")?);
    path.is_file().then_some(path)
}

fn bundled_keychain_export(executable: &Path) -> Option<PathBuf> {
    let path = executable
        .parent()?
        .parent()?
        .join("Resources")
        .join("keychain-export");
    path.is_file().then_some(path)
}

fn development_keychain_export(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .map(|directory| {
            directory
                .join("tools")
                .join("keychain-export")
                .join(".build")
                .join("release")
                .join("keychain-export")
        })
        .find(|path| path.is_file())
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

        let (_, first, first_digest) = collect(&profile, false, false).unwrap();
        let (_, second, second_digest) = collect(&profile, false, false).unwrap();
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

    #[test]
    fn dry_run_does_not_refresh_after_a_failed_probe() {
        let profile = ExecutionProfile {
            id: uuid::Uuid::new_v4(),
            org_id: None,
            name: "dry-run".into(),
            description: String::new(),
            credential_scopes: vec!["fixture".into()],
            collection: ExecutionProfileCollectionSpec {
                version: 1,
                probe: Some(ExecutionProfileCommand {
                    argv: vec!["false".into()],
                    interactive: false,
                }),
                refresh: Some(ExecutionProfileCommand {
                    argv: vec!["true".into()],
                    interactive: false,
                }),
                sources: vec![ExecutionProfileSource::File {
                    path: "/dev/null".into(),
                    target: ".tool/config".into(),
                }],
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
            health: ExecutionProfileHealth::Testing,
            last_error: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let error = collect(&profile, false, true).unwrap_err();
        assert!(error.to_string().contains("profile probe command 'false'"));
        assert!(error.to_string().contains("during dry run"));
    }

    #[test]
    fn missing_file_source_names_the_source_and_target() {
        let missing = std::env::temp_dir().join(format!(
            "runinator-profile-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let profile = ExecutionProfile {
            id: uuid::Uuid::new_v4(),
            org_id: None,
            name: "missing-file".into(),
            description: String::new(),
            credential_scopes: vec!["fixture".into()],
            collection: ExecutionProfileCollectionSpec {
                version: 1,
                probe: None,
                refresh: None,
                sources: vec![ExecutionProfileSource::File {
                    path: missing.to_string_lossy().into_owned(),
                    target: ".tool/config".into(),
                }],
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
            health: ExecutionProfileHealth::Testing,
            last_error: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        let error = collect(&profile, false, true).unwrap_err().to_string();
        assert!(error.contains("profile file source"));
        assert!(error.contains(missing.to_string_lossy().as_ref()));
        assert!(error.contains("target '.tool/config'"));
    }

    #[test]
    fn missing_directory_source_names_the_source_and_target() {
        let missing = std::env::temp_dir().join(format!(
            "runinator-profile-directory-missing-{}",
            uuid::Uuid::new_v4()
        ));
        let error = collect_directory(
            &mut BTreeMap::new(),
            &missing,
            ".tool/cache",
            &Pattern::new("*.json").unwrap(),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("profile directory source"));
        assert!(error.contains(missing.to_string_lossy().as_ref()));
        assert!(error.contains("target '.tool/cache'"));
    }

    #[test]
    fn status_error_is_bounded_for_the_profile_status_api() {
        let detail = status_error("desktop collection dry run failed", &"x".repeat(600));

        assert_eq!(detail.chars().count(), MAX_STATUS_ERROR_CHARS);
        assert!(detail.ends_with('…'));
    }

    #[test]
    fn collection_waits_for_a_running_connected_agent() {
        let mut status = runinator_worker::AgentStatus::default();
        status.connection = ConnectionState::Connected;
        assert!(!collection_can_run(&status));

        status.running = true;
        assert!(collection_can_run(&status));

        status.connection = ConnectionState::Connecting;
        assert!(!collection_can_run(&status));
    }

    #[test]
    fn bundled_keychain_export_uses_the_app_resources_directory() {
        let root = std::env::temp_dir().join(format!(
            "runinator-keychain-export-test-{}",
            uuid::Uuid::new_v4()
        ));
        let executable =
            root.join("Runinator Desktop Agent.app/Contents/MacOS/runinator-desktop-agent");
        let helper = root.join("Runinator Desktop Agent.app/Contents/Resources/keychain-export");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::create_dir_all(helper.parent().unwrap()).unwrap();
        fs::write(&executable, []).unwrap();
        fs::write(&helper, []).unwrap();

        assert_eq!(bundled_keychain_export(&executable), Some(helper));

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn cargo_build_stages_keychain_export() {
        assert!(cargo_built_keychain_export().is_some());
    }
}
