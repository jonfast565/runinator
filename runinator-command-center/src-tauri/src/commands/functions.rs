//! packaged functions and the rexrap console, for the desktop client.
//!
//! these mirror the web build's http registry entry for entry. the desktop app talks to the same web
//! service, so every command here is a thin proxy — the shapes and the reasoning live on the
//! backend, and duplicating either would give the two clients room to disagree.

use super::*;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::execution_profiles::ExecutionProfilePutRequest;

async fn execution_profile_client(
    state: &CommandCenterState,
) -> CommandResult<AsyncApiClient<StaticLocator>> {
    let base_url = state
        .service_url
        .read()
        .await
        .clone()
        .ok_or(CommandError::NoService)?;
    let client = state.client.read().await.clone();
    Ok(AsyncApiClient::with_client(
        StaticLocator::new(base_url),
        client,
    ))
}

fn api_error(error: runinator_api::ApiError) -> CommandError {
    CommandError::Unexpected(error.to_string())
}

fn json_value(value: impl serde::Serialize) -> CommandResult<Value> {
    serde_json::to_value(value).map_err(|error| CommandError::Unexpected(error.to_string()))
}

#[tauri::command]
pub async fn list_execution_profiles(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    json_value(
        execution_profile_client(&state)
            .await?
            .list_execution_profiles()
            .await
            .map_err(api_error)?,
    )
}

#[tauri::command]
pub async fn put_execution_profile(
    state: State<'_, CommandCenterState>,
    profile_id: String,
    profile: Value,
) -> CommandResult<Value> {
    let id = Uuid::parse_str(&profile_id)
        .map_err(|error| CommandError::Unexpected(error.to_string()))?;
    let request = serde_json::from_value::<ExecutionProfilePutRequest>(profile)
        .map_err(|error| CommandError::Unexpected(error.to_string()))?;
    json_value(
        execution_profile_client(&state)
            .await?
            .configure_execution_profile(id, &request)
            .await
            .map_err(api_error)?,
    )
}

#[tauri::command]
pub async fn delete_execution_profile(
    state: State<'_, CommandCenterState>,
    profile_id: String,
) -> CommandResult<Value> {
    let id = Uuid::parse_str(&profile_id)
        .map_err(|error| CommandError::Unexpected(error.to_string()))?;
    json_value(
        execution_profile_client(&state)
            .await?
            .delete_execution_profile(id)
            .await
            .map_err(api_error)?,
    )
}

#[tauri::command]
pub async fn rotate_execution_profile(
    state: State<'_, CommandCenterState>,
    profile_id: String,
) -> CommandResult<Value> {
    let id = Uuid::parse_str(&profile_id)
        .map_err(|error| CommandError::Unexpected(error.to_string()))?;
    json_value(
        execution_profile_client(&state)
            .await?
            .rotate_execution_profile(id)
            .await
            .map_err(api_error)?,
    )
}

#[tauri::command]
pub async fn test_execution_profile(
    state: State<'_, CommandCenterState>,
    profile_id: String,
) -> CommandResult<Value> {
    let id = Uuid::parse_str(&profile_id)
        .map_err(|error| CommandError::Unexpected(error.to_string()))?;
    json_value(
        execution_profile_client(&state)
            .await?
            .test_execution_profile(id)
            .await
            .map_err(api_error)?,
    )
}

// ---- packaged functions ----

#[tauri::command]
pub async fn list_function_packages(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "functions").await
}

#[tauri::command]
pub async fn fetch_function_package(
    state: State<'_, CommandCenterState>,
    package_name: String,
) -> CommandResult<Value> {
    get_json(&state, &format!("functions/{package_name}")).await
}

#[tauri::command]
pub async fn fetch_function_catalog(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "functions/catalog").await
}

/// store a package archive under the digest of its bytes.
///
/// the archive arrives base64-encoded because tauri's ipc is json; the web build posts the bytes
/// themselves. either way the server keeps them only if it does not already hold that digest.
#[tauri::command]
pub async fn upload_function_artifact(
    state: State<'_, CommandCenterState>,
    digest: String,
    base64: String,
) -> CommandResult<Value> {
    let bytes = decode_base64(&base64)?;
    crate::client::post_bytes(
        &state,
        &format!("function_artifacts/{digest}"),
        runinator_models::functions::ARTIFACT_MEDIA_TYPE,
        bytes,
    )
    .await
}

// ---- VM-native workflow input files ----

#[tauri::command]
pub async fn list_workflow_files(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "workflow_files").await
}

#[tauri::command]
pub async fn upload_workflow_file(
    state: State<'_, CommandCenterState>,
    path: String,
    mime_type: String,
    base64: String,
) -> CommandResult<Value> {
    let bytes = decode_base64(&base64)?;
    crate::client::post_bytes(
        &state,
        &format!(
            "workflow_files?path={}&mime_type={}",
            percent_encode(&path),
            percent_encode(&mime_type)
        ),
        &mime_type,
        bytes,
    )
    .await
}

#[tauri::command]
pub async fn stage_workflow_file(
    state: State<'_, CommandCenterState>,
    path: String,
    mime_type: String,
    base64: String,
) -> CommandResult<Value> {
    let bytes = decode_base64(&base64)?;
    crate::client::post_bytes(
        &state,
        &format!(
            "workflow_files/stage?path={}&mime_type={}",
            percent_encode(&path),
            percent_encode(&mime_type)
        ),
        &mime_type,
        bytes,
    )
    .await
}

#[tauri::command]
pub async fn archive_workflow_file(
    state: State<'_, CommandCenterState>,
    file_id: String,
) -> CommandResult<Value> {
    crate::client::delete(&state, &format!("workflow_files/{file_id}")).await
}

#[tauri::command]
pub async fn download_workflow_file(
    state: State<'_, CommandCenterState>,
    file_id: String,
) -> CommandResult<Vec<u8>> {
    crate::client::get_bytes(&state, &format!("workflow_files/{file_id}/content")).await
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[tauri::command]
pub async fn publish_function_version(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "functions", &request).await
}

#[tauri::command]
pub async fn restore_function_package(
    state: State<'_, CommandCenterState>,
    package_name: String,
) -> CommandResult<Value> {
    crate::client::post_empty(&state, &format!("functions/{package_name}/restore")).await
}

// standard base64, decoded by hand: one call site does not earn a dependency, and the alphabet has
// not moved since 1987.
pub(crate) fn decode_base64(text: &str) -> CommandResult<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let mut accumulator: u32 = 0;
    let mut bits = 0;
    for character in text.bytes() {
        if character == b'=' || character.is_ascii_whitespace() {
            continue;
        }
        let Some(value) = ALPHABET
            .iter()
            .position(|candidate| *candidate == character)
        else {
            return Err(CommandError::Unexpected(format!(
                "'{}' is not base64",
                character as char
            )));
        };
        accumulator = (accumulator << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((accumulator >> bits) as u8);
        }
    }
    Ok(bytes)
}

#[tauri::command]
pub async fn delete_function_package(
    state: State<'_, CommandCenterState>,
    package_name: String,
) -> CommandResult<Value> {
    delete(&state, &format!("functions/{package_name}")).await
}

#[tauri::command]
pub async fn set_function_alias(
    state: State<'_, CommandCenterState>,
    package_name: String,
    alias: String,
    version: Option<i64>,
    from_alias: Option<String>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("functions/{package_name}/aliases"),
        &json!({ "alias": alias, "version": version, "from_alias": from_alias }),
    )
    .await
}

#[tauri::command]
pub async fn delete_function_alias(
    state: State<'_, CommandCenterState>,
    package_name: String,
    alias: String,
) -> CommandResult<Value> {
    delete(&state, &format!("functions/{package_name}/aliases/{alias}")).await
}

#[tauri::command]
pub async fn invoke_function(
    state: State<'_, CommandCenterState>,
    package_name: String,
    export_name: String,
    input: Value,
    alias: Option<String>,
    version: Option<i64>,
) -> CommandResult<Value> {
    let mut path = format!("functions/{package_name}/{export_name}/invocations");
    // an alias wins over a version: the two select the same thing, and sending both would leave
    // the server to break the tie.
    if let Some(alias) = alias.filter(|alias| !alias.is_empty()) {
        path.push_str(&format!("?alias={alias}"));
    } else if let Some(version) = version {
        path.push_str(&format!("?version={version}"));
    }
    post_json(&state, &path, &input).await
}

// ---- the rexrap console ----

#[tauri::command]
pub async fn list_console_sessions(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "console/sessions").await
}

#[tauri::command]
pub async fn create_console_session(
    state: State<'_, CommandCenterState>,
    name: Option<String>,
) -> CommandResult<Value> {
    post_json(&state, "console/sessions", &json!({ "name": name })).await
}

#[tauri::command]
pub async fn fetch_console_session(
    state: State<'_, CommandCenterState>,
    session_id: Uuid,
) -> CommandResult<Value> {
    get_json(&state, &format!("console/sessions/{session_id}")).await
}

#[tauri::command]
pub async fn rename_console_session(
    state: State<'_, CommandCenterState>,
    session_id: Uuid,
    name: String,
) -> CommandResult<Value> {
    patch_json(
        &state,
        &format!("console/sessions/{session_id}"),
        &json!({ "name": name }),
    )
    .await
}

#[tauri::command]
pub async fn delete_console_session(
    state: State<'_, CommandCenterState>,
    session_id: Uuid,
) -> CommandResult<Value> {
    delete(&state, &format!("console/sessions/{session_id}")).await
}

#[tauri::command]
pub async fn clear_console_session(
    state: State<'_, CommandCenterState>,
    session_id: Uuid,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("console/sessions/{session_id}/clear"),
        &json!({}),
    )
    .await
}

#[tauri::command]
pub async fn create_console_cell(
    state: State<'_, CommandCenterState>,
    session_id: Uuid,
    source: String,
    label: Option<String>,
    position: Option<i64>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("console/sessions/{session_id}/cells"),
        &json!({ "source": source, "label": label, "position": position }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_console_cell(
    state: State<'_, CommandCenterState>,
    cell_id: Uuid,
) -> CommandResult<Value> {
    get_json(&state, &format!("console/cells/{cell_id}")).await
}

#[tauri::command]
pub async fn update_console_cell(
    state: State<'_, CommandCenterState>,
    cell_id: Uuid,
    source: String,
    label: Option<String>,
    position: Option<i64>,
) -> CommandResult<Value> {
    patch_json(
        &state,
        &format!("console/cells/{cell_id}"),
        &json!({ "source": source, "label": label, "position": position }),
    )
    .await
}

#[tauri::command]
pub async fn delete_console_cell(
    state: State<'_, CommandCenterState>,
    cell_id: Uuid,
) -> CommandResult<Value> {
    delete(&state, &format!("console/cells/{cell_id}")).await
}

#[tauri::command]
pub async fn run_console_cell(
    state: State<'_, CommandCenterState>,
    cell_id: Uuid,
) -> CommandResult<Value> {
    post_empty(&state, &format!("console/cells/{cell_id}/run")).await
}
