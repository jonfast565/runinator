//! packaged functions and the rexrap console, for the desktop client.
//!
//! these mirror the web build's http registry entry for entry. the desktop app talks to the same web
//! service, so every command here is a thin proxy — the shapes and the reasoning live on the
//! backend, and duplicating either would give the two clients room to disagree.

use super::*;

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
fn decode_base64(text: &str) -> CommandResult<Vec<u8>> {
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
