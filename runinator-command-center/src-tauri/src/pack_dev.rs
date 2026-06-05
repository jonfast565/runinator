use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use runinator_models::{
    api_routes::API_PACKS_IMPORT,
    bundles::{PackImportResult, SecretBundle},
    settings::SettingSummary,
    value::Value,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger},
};
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
    if source.extension().and_then(|ext| ext.to_str()) != Some("wdl") {
        return Err(command_error(
            "only .wdl source files can be saved from the dev panel",
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
    let body = runinator_utilities::pack::build_pack_zip(&bundle, settings.as_ref())
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let url = build_state_url(&state, API_PACKS_IMPORT).await?;
    let response = state
        .client
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
    if path.is_dir() {
        return true;
    }
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("wdl") | Some("wdlp")
    )
}

fn pack_source_files(path: &Path) -> CommandResult<Vec<DevPackFile>> {
    let mut files = Vec::new();

    if path.is_dir() {
        files.extend(wdl_directory_paths(path)?);
        if let Some(settings_path) = pack_settings_path(path)? {
            files.push(settings_path);
        }
        return Ok(source_file_summaries(files));
    }

    match path.extension().and_then(|ext| ext.to_str()) {
        Some("wdlp") => {
            files.push(path.to_path_buf());
            files.extend(wdl_pack_manifest_paths(path)?);
            if let Some(settings_path) = pack_settings_path(path)? {
                files.push(settings_path);
            }
        }
        _ => files.push(path.to_path_buf()),
    }

    Ok(source_file_summaries(files))
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
        Some("wdlp") => "manifest".into(),
        Some("wdl") => "workflow".into(),
        Some("wdls") => "settings".into(),
        Some("json") => "json".into(),
        _ => "file".into(),
    }
}

fn load_pack_settings(path: &Path) -> CommandResult<Option<SecretBundle>> {
    let Some(settings_path) = pack_settings_path(path)? else {
        return Ok(None);
    };
    parse_settings_file(&settings_path).map(Some)
}

fn parse_settings_file(path: &Path) -> CommandResult<SecretBundle> {
    let data = fs::read_to_string(path).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let mut bundle: SecretBundle = match path.extension().and_then(|ext| ext.to_str()) {
        Some("wdls") => runinator_wdl::parse_secrets_str(&data).map_err(|err| {
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
        for name in ["settings.wdls", "settings.json"] {
            let candidate = path.join(name);
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
        return Ok(None);
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("wdlp") {
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
    if !is_pack_source(path) {
        return Err(command_error(format!(
            "unsupported pack source: {}",
            path.display()
        )));
    }
    if path.is_dir() {
        return load_wdl_directory(path);
    }
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("wdlp") => load_wdl_pack_manifest(path),
        Some("wdl") => {
            let data = fs::read_to_string(path)
                .map_err(|err| CommandError::Unexpected(err.to_string()))?;
            let definition = compile_wdl(path, &data, 1)?;
            Ok(WorkflowBundle {
                workflows: vec![definition],
                triggers: Vec::new(),
            })
        }
        _ => Err(command_error(format!(
            "unsupported pack source: {}",
            path.display()
        ))),
    }
}

fn compile_wdl(path: &Path, data: &str, default_version: i64) -> CommandResult<WorkflowDefinition> {
    let options = runinator_wdl::CompileOptions {
        enabled: true,
        default_version,
    };
    let formatted = runinator_wdl::format_str(data).map_err(|err| {
        command_error(format!(
            "failed to format {} before import:\n{}",
            path.display(),
            err.render(data)
        ))
    })?;
    let mut definition = runinator_wdl::compile_str(&formatted, &options).map_err(|err| {
        command_error(format!(
            "failed to compile {}:\n{}",
            path.display(),
            err.render(&formatted)
        ))
    })?;
    definition.updated_at = file_modified(path);
    Ok(definition)
}

fn load_wdl_directory(dir: &Path) -> CommandResult<WorkflowBundle> {
    let wdl_paths = wdl_directory_paths(dir)?;
    if wdl_paths.is_empty() {
        return Err(command_error(format!(
            "no .wdl files found in {}",
            dir.display()
        )));
    }
    let mut workflows = Vec::with_capacity(wdl_paths.len());
    for wdl_path in &wdl_paths {
        let data = fs::read_to_string(wdl_path)
            .map_err(|err| CommandError::Unexpected(err.to_string()))?;
        workflows.push(compile_wdl(wdl_path, &data, 1)?);
    }
    Ok(WorkflowBundle {
        workflows,
        triggers: Vec::new(),
    })
}

fn wdl_directory_paths(dir: &Path) -> CommandResult<Vec<PathBuf>> {
    let mut wdl_paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(|err| CommandError::Unexpected(err.to_string()))? {
        let entry_path = entry
            .map_err(|err| CommandError::Unexpected(err.to_string()))?
            .path();
        if entry_path.extension().and_then(|ext| ext.to_str()) == Some("wdl") {
            wdl_paths.push(entry_path);
        }
    }
    wdl_paths.sort();
    Ok(wdl_paths)
}

fn load_wdl_pack_manifest(path: &Path) -> CommandResult<WorkflowBundle> {
    let data = fs::read_to_string(path).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let manifest: Value =
        serde_json::from_str(&data).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let version = manifest
        .get("version")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(1);
    let paths = wdl_pack_manifest_paths_from_value(path, &manifest)?;

    let mut workflows = Vec::with_capacity(paths.len());
    for wdl_path in paths {
        let source = fs::read_to_string(&wdl_path)
            .map_err(|err| CommandError::Unexpected(err.to_string()))?;
        workflows.push(compile_wdl(&wdl_path, &source, version)?);
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

fn wdl_pack_manifest_paths(path: &Path) -> CommandResult<Vec<PathBuf>> {
    let data = fs::read_to_string(path).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let manifest: Value =
        serde_json::from_str(&data).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    wdl_pack_manifest_paths_from_value(path, &manifest)
}

fn wdl_pack_manifest_paths_from_value(
    path: &Path,
    manifest: &Value,
) -> CommandResult<Vec<PathBuf>> {
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let entries = manifest
        .get("workflows")
        .and_then(Value::as_array)
        .ok_or_else(|| command_error("wdl pack manifest missing 'workflows' array"))?;

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
