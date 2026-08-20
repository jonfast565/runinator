use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use runinator_models::{
    api_routes::API_PACKS_IMPORT,
    bundles::{PackImportResult, SecretBundle},
    semver::SemVer,
    settings::SettingSummary,
    value::Value,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger},
};
use runinator_rexrap::WorkflowSignature;
use serde::Serialize;
use tauri::State;

use crate::{
    client::{build_state_url, handle_response},
    error::{CommandError, CommandResult},
    state::CommandCenterState,
};

#[derive(Debug, Clone, Serialize)]
pub struct DevPackFile {
    pub path: String,
    pub kind: String,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevPackInspectResult {
    pub path: String,
    pub files: Vec<DevPackFile>,
    pub workflows: Vec<WorkflowDefinition>,
    pub triggers: Vec<WorkflowTrigger>,
    pub settings_count: usize,
    // identities (no values) of the setting slots the pack would write on import.
    pub settings: Vec<SettingSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevPackApplyResult {
    pub path: String,
    pub files: Vec<DevPackFile>,
    pub imported: PackImportResult,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevPackTextFile {
    pub path: String,
    pub content: String,
    pub modified_at: Option<DateTime<Utc>>,
}

#[tauri::command]
pub fn inspect_dev_pack(
    path: String,
    skip_settings: Option<bool>,
) -> CommandResult<DevPackInspectResult> {
    let source = PathBuf::from(path);
    let bundle = load_workflow_bundle(&source)?;
    let settings = if skip_settings.unwrap_or(false) {
        None
    } else {
        load_pack_settings(&source)?
    };
    let setting_summaries = settings
        .as_ref()
        .map(|bundle| {
            bundle
                .secrets
                .iter()
                .map(|entry| SettingSummary {
                    scope: entry.scope.clone(),
                    name: entry.name.clone(),
                    kind: entry.kind,
                    expires_at: entry.expires_at,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(DevPackInspectResult {
        path: source.display().to_string(),
        files: pack_source_files(&source)?,
        workflows: bundle.workflows,
        triggers: bundle.triggers,
        settings_count: setting_summaries.len(),
        settings: setting_summaries,
    })
}

#[tauri::command]
pub fn read_dev_pack_file(path: String) -> CommandResult<DevPackTextFile> {
    let source = PathBuf::from(path);
    Ok(DevPackTextFile {
        path: source.display().to_string(),
        content: fs::read_to_string(&source)
            .map_err(|err| CommandError::Unexpected(err.to_string()))?,
        modified_at: file_modified(&source),
    })
}

#[tauri::command]
pub fn write_dev_pack_file(path: String, content: String) -> CommandResult<DevPackTextFile> {
    let source = PathBuf::from(path);
    if source.extension().and_then(|ext| ext.to_str()) != Some("rrx") {
        return Err(command_error(
            "only .rrx source files can be saved from the dev panel",
        ));
    }
    fs::write(&source, content).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    read_dev_pack_file(source.display().to_string())
}

#[tauri::command]
pub async fn apply_dev_pack(
    state: State<'_, CommandCenterState>,
    path: String,
    skip_settings: Option<bool>,
) -> CommandResult<DevPackApplyResult> {
    let source = PathBuf::from(path);
    let bundle = load_workflow_bundle(&source)?;
    let settings = if skip_settings.unwrap_or(false) {
        None
    } else {
        load_pack_settings(&source)?
    };
    // desktop dev re-apply pushes workflows + settings; pipelines are pack-managed via ctl apply.
    let body = runinator_utilities::pack::build_pack_zip(&bundle, settings.as_ref(), None)
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let mut url = build_state_url(&state, API_PACKS_IMPORT).await?;
    // an explicit dev re-apply is authoritative: update existing items in place.
    url.set_query(Some("overwrite=true"));
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .header(reqwest::header::CONTENT_TYPE, "application/zip")
        .body(body)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(DevPackApplyResult {
        path: source.display().to_string(),
        files: pack_source_files(&source)?,
        imported: response.json::<PackImportResult>().await?,
    })
}

fn command_error(message: impl Into<String>) -> CommandError {
    CommandError::Unexpected(message.into())
}

fn file_modified(path: &Path) -> Option<DateTime<Utc>> {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

fn is_pack_source(path: &Path) -> bool {
    runinator_pack::source::is_pack_source(path)
}

fn pack_source_files(path: &Path) -> CommandResult<Vec<DevPackFile>> {
    let files = runinator_pack::source::pack_source_files(path)
        .map_err(|err| command_error(err.to_string()))?;
    Ok(source_file_summaries(files))
}

fn extend_rexrap_includes(files: &mut Vec<PathBuf>) {
    let rexrap_files = files
        .iter()
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rexrap"))
        .cloned()
        .collect::<Vec<_>>();
    for path in rexrap_files {
        let Ok(data) = fs::read_to_string(&path) else {
            continue;
        };
        let source_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let Ok(included) = runinator_rexrap::included_file_paths(&data, source_dir) else {
            continue;
        };
        files.extend(included);
    }
}

fn source_file_summaries(mut paths: Vec<PathBuf>) -> Vec<DevPackFile> {
    paths.sort();
    paths.dedup();
    paths
        .into_iter()
        .map(|path| {
            let metadata = fs::metadata(&path).ok();
            DevPackFile {
                kind: source_file_kind(&path),
                path: path.display().to_string(),
                size_bytes: metadata.as_ref().map(|meta| meta.len()),
                modified_at: metadata
                    .as_ref()
                    .and_then(|meta| meta.modified().ok())
                    .map(DateTime::<Utc>::from),
            }
        })
        .collect()
}

fn source_file_kind(path: &Path) -> String {
    if path.is_dir() {
        return "directory".into();
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rrx") => "rrx source".into(),
        Some("json") => "json".into(),
        _ => "file".into(),
    }
}

fn load_pack_settings(path: &Path) -> CommandResult<Option<SecretBundle>> {
    runinator_pack::source::load_pack_settings(path).map_err(|err| command_error(err.to_string()))
}

fn parse_settings_file(path: &Path) -> CommandResult<SecretBundle> {
    let data = fs::read_to_string(path).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let mut bundle: SecretBundle = match path.extension().and_then(|ext| ext.to_str()) {
        Some("rexraps") => runinator_rexrap::parse_secrets_str(&data).map_err(|err| {
            command_error(format!(
                "failed to parse {}:\n{}",
                path.display(),
                err.render(&data)
            ))
        })?,
        _ => {
            serde_json::from_str(&data).map_err(|err| CommandError::Unexpected(err.to_string()))?
        }
    };
    if let Some(modified) = file_modified(path) {
        for entry in &mut bundle.secrets {
            entry.updated_at.get_or_insert(modified);
        }
    }
    Ok(bundle)
}

fn pack_settings_path(path: &Path) -> CommandResult<Option<PathBuf>> {
    if path.is_dir() {
        for name in ["settings.rexraps", "settings.json"] {
            let candidate = path.join(name);
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
        return Ok(None);
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("rexrapm") {
        return Ok(None);
    }
    let data = fs::read_to_string(path).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let manifest: Value =
        serde_json::from_str(&data).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(manifest
        .get("settings")
        .and_then(Value::as_str)
        .map(|rel| base_dir.join(rel)))
}

fn load_workflow_bundle(path: &Path) -> CommandResult<WorkflowBundle> {
    runinator_pack::source::load_workflow_bundle(path).map_err(|err| command_error(err.to_string()))
}

fn compile_rexrap_all(
    path: &Path,
    data: &str,
    default_version: SemVer,
) -> CommandResult<Vec<WorkflowDefinition>> {
    compile_rexrap_all_with_signatures(path, data, default_version, &[])
}

fn compile_rexrap_all_with_signatures(
    path: &Path,
    data: &str,
    default_version: SemVer,
    workflow_signatures: &[WorkflowSignature],
) -> CommandResult<Vec<WorkflowDefinition>> {
    let options = runinator_rexrap::CompileOptions {
        enabled: true,
        default_version,
        source_dir: path.parent().map(Path::to_path_buf),
        providers: runinator_provider_catalog::metadata(),
        workflow_signatures: workflow_signatures.to_vec(),
        ..runinator_rexrap::CompileOptions::default()
    };
    let formatted = runinator_rexrap::format_str(data).map_err(|err| {
        command_error(format!(
            "failed to format {} before import:\n{}",
            path.display(),
            err.render(data)
        ))
    })?;
    let mut definitions =
        runinator_rexrap::compile_all_str(&formatted, &options).map_err(|err| {
            command_error(format!(
                "failed to compile {}:\n{}",
                path.display(),
                err.render(&formatted)
            ))
        })?;
    let updated_at = file_modified(path);
    for definition in &mut definitions {
        definition.updated_at = updated_at;
    }
    Ok(definitions)
}

fn collect_workflow_signatures(paths: &[PathBuf]) -> CommandResult<Vec<WorkflowSignature>> {
    collect_workflow_signatures_with_current(paths, None, None)
}

fn collect_workflow_signatures_with_current(
    paths: &[PathBuf],
    current_path: Option<&Path>,
    current_source: Option<&str>,
) -> CommandResult<Vec<WorkflowSignature>> {
    let mut signatures = Vec::new();
    for path in paths {
        let data;
        let source = if Some(path.as_path()) == current_path {
            match current_source {
                Some(source) => source,
                None => {
                    data = fs::read_to_string(path)
                        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
                    &data
                }
            }
        } else {
            data = fs::read_to_string(path)
                .map_err(|err| CommandError::Unexpected(err.to_string()))?;
            &data
        };
        let mut source_signatures = runinator_rexrap::workflow_signature_from_source(source)
            .map_err(|err| {
                command_error(format!(
                    "failed to read workflow signature from {}:\n{}",
                    path.display(),
                    err.render(source)
                ))
            })?;
        signatures.append(&mut source_signatures);
    }
    Ok(signatures)
}

pub(crate) fn rexrap_context_workflow_signatures(
    path: &Path,
    current_source: Option<&str>,
) -> CommandResult<Vec<WorkflowSignature>> {
    runinator_pack::source::rexrap_context_workflow_signatures(path, current_source)
        .map_err(|err| command_error(err.to_string()))
}

fn load_rexrap_directory(dir: &Path) -> CommandResult<WorkflowBundle> {
    let rexrap_paths = rexrap_directory_paths(dir)?;
    if rexrap_paths.is_empty() {
        return Err(command_error(format!(
            "no .rexrap files found in {}",
            dir.display()
        )));
    }
    let workflow_signatures = collect_workflow_signatures(&rexrap_paths)?;
    let mut workflows = Vec::with_capacity(rexrap_paths.len());
    for rexrap_path in &rexrap_paths {
        let data = fs::read_to_string(rexrap_path)
            .map_err(|err| CommandError::Unexpected(err.to_string()))?;
        workflows.extend(compile_rexrap_all_with_signatures(
            rexrap_path,
            &data,
            SemVer::default(),
            &workflow_signatures,
        )?);
    }
    Ok(WorkflowBundle {
        workflows,
        triggers: Vec::new(),
    })
}

fn rexrap_directory_paths(dir: &Path) -> CommandResult<Vec<PathBuf>> {
    let mut rexrap_paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| CommandError::Unexpected(err.to_string()))? {
        let entry_path = entry
            .map_err(|err| CommandError::Unexpected(err.to_string()))?
            .path();
        if entry_path.extension().and_then(|ext| ext.to_str()) == Some("rexrap") {
            rexrap_paths.push(entry_path);
        }
    }
    rexrap_paths.sort();
    Ok(rexrap_paths)
}

fn load_rexrap_pack_manifest(path: &Path) -> CommandResult<WorkflowBundle> {
    let data = fs::read_to_string(path).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let manifest: Value =
        serde_json::from_str(&data).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let version = manifest
        .get("version")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<SemVer>().ok())
                .or_else(|| {
                    v.as_i64()
                        .map(|major| SemVer::new(major.max(0) as u64, 0, 0))
                })
        })
        .unwrap_or_default();
    let paths = rexrap_pack_manifest_paths_from_value(path, &manifest)?;

    let workflow_signatures = collect_workflow_signatures(&paths)?;
    let mut workflows = Vec::with_capacity(paths.len());
    for rexrap_path in paths {
        let source = fs::read_to_string(&rexrap_path)
            .map_err(|err| CommandError::Unexpected(err.to_string()))?;
        workflows.extend(compile_rexrap_all_with_signatures(
            &rexrap_path,
            &source,
            version,
            &workflow_signatures,
        )?);
    }

    let triggers = match manifest.get("triggers").cloned() {
        Some(value) if !value.is_null() => {
            serde_json::from_value::<Vec<WorkflowTrigger>>(value.into())
                .map_err(|err| CommandError::Unexpected(err.to_string()))?
        }
        _ => Vec::new(),
    };

    Ok(WorkflowBundle {
        workflows,
        triggers,
    })
}

fn rexrap_pack_manifest_paths(path: &Path) -> CommandResult<Vec<PathBuf>> {
    let data = fs::read_to_string(path).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let manifest: Value =
        serde_json::from_str(&data).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    rexrap_pack_manifest_paths_from_value(path, &manifest)
}

fn rexrap_pack_manifest_paths_from_value(
    path: &Path,
    manifest: &Value,
) -> CommandResult<Vec<PathBuf>> {
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let entries = manifest
        .get("workflows")
        .and_then(Value::as_array)
        .ok_or_else(|| command_error("rexrap pack manifest missing 'workflows' array"))?;

    let mut paths = Vec::with_capacity(entries.len());
    for entry in entries {
        let rel = entry
            .as_str()
            .or_else(|| entry.get("path").and_then(Value::as_str))
            .ok_or_else(|| {
                command_error("each manifest workflow entry must be a path string or have a 'path'")
            })?;
        paths.push(base_dir.join(rel));
    }
    paths.sort();
    Ok(paths)
}
