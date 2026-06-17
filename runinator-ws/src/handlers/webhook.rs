use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;

use crate::events::{AppEvent, EventSender, emit, emit_workflow_run};
use crate::models::{ApiResponse, WebhookSignalRequest, WebhookWakeRequest};
use crate::repository;
use crate::responses::{api_error, not_found, task_response_success};
use crate::websocket::merge_json;

pub(crate) async fn webhook_wake<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<WebhookWakeRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    let workflow_run =
        match repository::fetch_workflow_run(db.as_ref(), request.workflow_run_id).await {
            Ok(Some(workflow_run)) => workflow_run,
            Ok(None) => {
                return not_found(format!(
                    "Workflow run {} not found",
                    request.workflow_run_id
                ));
            }
            Err(err) => return api_error(err.to_string()),
        };
    let (run, node_runs) = workflow_run;
    let node_id = request
        .node_id
        .clone()
        .or(run.active_node_id)
        .unwrap_or_default();
    let Some(node_run) = node_runs
        .iter()
        .filter(|node_run| node_run.node_id == node_id)
        .max_by_key(|node_run| node_run.id)
    else {
        return task_response_success("Webhook wake recorded");
    };

    let mut state = node_run.state.clone();
    merge_json(&mut state, request.state);
    if let Some(status) = request.status
        && let Some(object) = state.as_object_mut()
    {
        object.insert("status".into(), status.into());
    }
    if let Err(err) = repository::update_workflow_node_run(
        db.as_ref(),
        node_run.id,
        runinator_models::workflows::WorkflowStatus::Waiting,
        None,
        None,
        None,
        Some(state.clone()),
        Some("webhook_wake".into()),
        None,
    )
    .await
    {
        return api_error(err.to_string());
    }
    if let Err(err) = repository::update_workflow_run_status(
        db.as_ref(),
        request.workflow_run_id,
        runinator_models::workflows::WorkflowStatus::Waiting,
        Some(node_id),
        Some(state),
        request.message,
    )
    .await
    {
        return api_error(err.to_string());
    }
    emit_workflow_run(&events, request.workflow_run_id);
    task_response_success("Webhook wake recorded")
}

/// route an inbound signal to a parked node by correlation key, without a run id. external systems
/// (github/jira/ci) authenticate as a service principal and post `{ name, correlation_key, payload }`.
pub(crate) async fn webhook_signal<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<WebhookSignalRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::deliver_signal_by_correlation(
        db.as_ref(),
        request.name,
        request.correlation_key,
        request.payload,
    )
    .await
    {
        Ok(response) => {
            emit(&events, AppEvent::WorkflowRunActivity);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(response)))
        }
        Err(err) => api_error(err.to_string()),
    }
}
