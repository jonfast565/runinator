use runinator_models::orchestration::{
    AdapterDefinition, AdapterKindMetadata, AdapterRevision, ExternalOperation,
    OrchestrationBinding, OrchestrationCommand, OrchestrationEpoch, OrchestrationEventReduction,
    OrchestrationEvidence,
};
use runinator_models::workspaces::WorkspaceLease;
use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

use crate::{
    client::{delete, get_json, post_json},
    error::CommandResult,
    state::CommandCenterState,
};

#[tauri::command]
pub async fn fetch_orchestrations(
    state: State<'_, CommandCenterState>,
    filters: Option<Value>,
) -> CommandResult<Vec<OrchestrationBinding>> {
    let query = {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        if let Some(filters) = filters.and_then(|value| value.as_object().cloned()) {
            for (key, value) in filters {
                if !value.is_null() {
                    let rendered = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    serializer.append_pair(&key, &rendered);
                }
            }
        }
        serializer.finish()
    };
    let path = if query.is_empty() {
        "orchestrations".into()
    } else {
        format!("orchestrations?{query}")
    };
    get_json(&state, &path).await
}

#[tauri::command]
pub async fn fetch_orchestration(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
) -> CommandResult<OrchestrationBinding> {
    get_json(&state, &format!("orchestrations/{orchestration_id}")).await
}

#[tauri::command]
pub async fn fetch_orchestration_epochs(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
) -> CommandResult<Vec<OrchestrationEpoch>> {
    get_json(&state, &format!("orchestrations/{orchestration_id}/epochs")).await
}

#[tauri::command]
pub async fn fetch_orchestration_events(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
) -> CommandResult<Vec<OrchestrationEventReduction>> {
    get_json(&state, &format!("orchestrations/{orchestration_id}/events")).await
}

#[tauri::command]
pub async fn fetch_orchestration_evidence(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
) -> CommandResult<Vec<OrchestrationEvidence>> {
    get_json(
        &state,
        &format!("orchestrations/{orchestration_id}/evidence"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_orchestration_commands(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
) -> CommandResult<Vec<OrchestrationCommand>> {
    get_json(
        &state,
        &format!("orchestrations/{orchestration_id}/commands"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_orchestration_workspaces(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
) -> CommandResult<Vec<WorkspaceLease>> {
    get_json(
        &state,
        &format!("orchestrations/{orchestration_id}/workspaces"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_external_operations(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
) -> CommandResult<Vec<ExternalOperation>> {
    get_json(
        &state,
        &format!("orchestrations/{orchestration_id}/operations"),
    )
    .await
}

#[tauri::command]
pub async fn resolve_external_operation(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
    operation_id: Uuid,
    resolution: String,
    reason: String,
    receipt: Option<Value>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("orchestrations/{orchestration_id}/operations/{operation_id}/resolve"),
        &json!({
            "resolution": resolution,
            "reason": reason,
            "receipt": receipt.unwrap_or(Value::Null),
        }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_adapter_kinds(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<AdapterKindMetadata>> {
    get_json(&state, "orchestrations/adapters/kinds").await
}

#[tauri::command]
pub async fn fetch_adapters(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<AdapterDefinition>> {
    get_json(&state, "orchestrations/adapters").await
}

#[tauri::command]
pub async fn fetch_adapter(
    state: State<'_, CommandCenterState>,
    adapter_id: Uuid,
) -> CommandResult<AdapterDefinition> {
    get_json(&state, &format!("orchestrations/adapters/{adapter_id}")).await
}

#[tauri::command]
pub async fn fetch_adapter_revisions(
    state: State<'_, CommandCenterState>,
    adapter_id: Uuid,
) -> CommandResult<Vec<AdapterRevision>> {
    get_json(
        &state,
        &format!("orchestrations/adapters/{adapter_id}/revisions"),
    )
    .await
}

#[tauri::command]
pub async fn apply_adapter(
    state: State<'_, CommandCenterState>,
    adapter: Value,
    adapter_id: Option<Uuid>,
) -> CommandResult<Value> {
    let path = adapter_id
        .map(|id| format!("orchestrations/adapters/{id}"))
        .unwrap_or_else(|| "orchestrations/adapters".into());
    post_json(&state, &path, &adapter).await
}

#[tauri::command]
pub async fn set_adapter_enabled(
    state: State<'_, CommandCenterState>,
    adapter_id: Uuid,
    enabled: bool,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("orchestrations/adapters/{adapter_id}/enabled"),
        &json!({ "enabled": enabled }),
    )
    .await
}

#[tauri::command]
pub async fn delete_adapter(
    state: State<'_, CommandCenterState>,
    adapter_id: Uuid,
) -> CommandResult<Value> {
    delete(&state, &format!("orchestrations/adapters/{adapter_id}")).await
}

#[tauri::command]
pub async fn test_adapter(
    state: State<'_, CommandCenterState>,
    adapter_id: Uuid,
    headers: Option<Value>,
    body_base64: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("orchestrations/adapters/{adapter_id}/test"),
        &json!({ "headers": headers.unwrap_or_else(|| json!({})), "body_base64": body_base64 }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_adapter_health(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "orchestrations/adapters/health").await
}

#[tauri::command]
pub async fn reload_adapter_host(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    post_json(&state, "orchestrations/adapters/reload", &json!({})).await
}

#[tauri::command]
pub async fn send_orchestration_intent(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
    intent: String,
    reason: String,
    payload: Option<Value>,
    idempotency_key: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("orchestrations/{orchestration_id}/intents"),
        &json!({
            "intent": intent, "reason": reason, "payload": payload.unwrap_or_else(|| json!({})),
            "idempotency_key": idempotency_key,
        }),
    )
    .await
}

#[tauri::command]
pub async fn requeue_orchestration(
    state: State<'_, CommandCenterState>,
    orchestration_id: Uuid,
    reason: String,
    idempotency_key: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("orchestrations/{orchestration_id}/requeue"),
        &json!({ "reason": reason, "idempotency_key": idempotency_key }),
    )
    .await
}
