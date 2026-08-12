use super::*;

#[tauri::command]
pub async fn fetch_resource_records(
    state: State<'_, CommandCenterState>,
    endpoint: String,
) -> CommandResult<Vec<Value>> {
    get_json(&state, &endpoint).await
}

#[tauri::command]
pub async fn fetch_providers(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<ProviderMetadata>> {
    get_json(&state, "providers").await
}

#[tauri::command]
pub async fn fetch_node_kinds(state: State<'_, CommandCenterState>) -> CommandResult<Vec<Value>> {
    get_json(&state, "node-kinds").await
}

#[tauri::command]
pub async fn fetch_trigger_kinds(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<Value>> {
    get_json(&state, "trigger-kinds").await
}

#[tauri::command]
pub async fn fetch_enum_catalogs(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<Value>> {
    get_json(&state, "catalog/enums").await
}

#[tauri::command]
pub async fn fetch_replicas(
    state: State<'_, CommandCenterState>,
) -> CommandResult<ReplicaListResponse> {
    get_json(&state, "replicas").await
}

#[tauri::command]
pub async fn create_agent_directive(
    state: State<'_, CommandCenterState>,
    replica_id: String,
    kind: Value,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("replicas/{replica_id}/directives"),
        &serde_json::json!({ "kind": kind, "expires_in_seconds": 300 }),
    )
    .await
}

#[tauri::command]
pub async fn list_agent_directives(
    state: State<'_, CommandCenterState>,
    replica_id: String,
    limit: i64,
) -> CommandResult<Vec<Value>> {
    get_json(
        &state,
        &format!("replicas/{replica_id}/directives?limit={limit}"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_node_backends(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "nodes/backends").await
}

#[tauri::command]
pub async fn fetch_nodes(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "nodes").await
}

#[tauri::command]
pub async fn scale_nodes(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "nodes/scale", &request).await
}

#[tauri::command]
pub async fn stop_node(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "nodes/stop", &request).await
}

// --- organizations (tenants), membership, resource allocation, and billing ---
