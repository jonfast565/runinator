use runinator_models::orchestration::{
    OrchestrationBinding, OrchestrationCommand, OrchestrationEpoch, OrchestrationEventReduction,
    OrchestrationEvidence,
};
use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

use crate::{
    client::{get_json, post_json},
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
