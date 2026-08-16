//! packaged functions and the wdl console, for the desktop client.
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
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("functions/{package_name}/{export_name}/invocations"),
        &input,
    )
    .await
}

// ---- the wdl console ----

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
