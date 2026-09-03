use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use runinator_models::{
    api_routes::API_PACKS_IMPORT,
    bundles::{PackImportResult, SettingsBundle},
    settings::SettingSummary,
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

/// Upload a pack which has already been compiled into the shared pack ZIP wire format.
/// Compilation intentionally remains on the client (the desktop dev panel and runinatorctl do it
/// from local source); this command only proxies bytes selected in the Command Center UI.
#[tauri::command]
pub async fn import_pack_archive(
    state: State<'_, CommandCenterState>,
    base64: String,
    overwrite: Option<bool>,
) -> CommandResult<PackImportResult> {
    let bytes = crate::commands::decode_base64(&base64)?;
    let path = format!(
        "{API_PACKS_IMPORT}?overwrite={}",
        overwrite.unwrap_or(false)
    );
    let value = crate::client::post_bytes(&state, &path, "application/zip", bytes).await?;
    serde_json::from_value(value).map_err(|err| CommandError::Unexpected(err.to_string()))
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
                .settings
                .iter()
                .map(|entry| SettingSummary {
                    // Authored pack settings have no server identity until import. Completion only
                    // needs their paths, so use the nil UUID as an explicit provisional sentinel.
                    id: uuid::Uuid::nil(),
                    org_id: None,
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
    let body = runinator_pack_wire::pack::build_pack_zip(&bundle, settings.as_ref(), None)
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

fn pack_source_files(path: &Path) -> CommandResult<Vec<DevPackFile>> {
    let files = runinator_pack::source::pack_source_files(path)
        .map_err(|err| command_error(err.to_string()))?;
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
        Some("rrx") => "rrx source".into(),
        Some("json") => "json".into(),
        _ => "file".into(),
    }
}

fn load_pack_settings(path: &Path) -> CommandResult<Option<SettingsBundle>> {
    runinator_pack::source::load_pack_settings(path).map_err(|err| command_error(err.to_string()))
}

fn load_workflow_bundle(path: &Path) -> CommandResult<WorkflowBundle> {
    runinator_pack::source::load_workflow_bundle(path).map_err(|err| command_error(err.to_string()))
}

pub(crate) fn rexrap_context_workflow_signatures(
    path: &Path,
    current_source: Option<&str>,
) -> CommandResult<Vec<WorkflowSignature>> {
    runinator_pack::source::rexrap_context_workflow_signatures(path, current_source)
        .map_err(|err| command_error(err.to_string()))
}
