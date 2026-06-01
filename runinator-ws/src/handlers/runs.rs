use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::orchestration::{ReadyNodeClaimRequest, ReadyNodeProcessRequest};
use runinator_models::runs::NewRunChunk;
use serde::Deserialize;

use crate::events::{AppEvent, EventSender, emit, emit_task_run, emit_workflow_run};
use crate::models::{
    self, ApiResponse, RunStatusQuery, RunStatusRequest, SchedulerRunClaimReleaseRequest,
    SchedulerRunClaimRenewRequest, SchedulerRunClaimRequest, WorkflowRunRequest,
    WorkflowRunStatusQuery, WorkflowRunStatusRequest, WorkflowTriggerRunRequest,
};
use crate::repository;
use crate::responses::{api_error, bad_request, not_found};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ChunkQuery {
    pub(crate) cursor: Option<i64>,
    pub(crate) limit: Option<i64>,
}

pub(crate) async fn create_workflow_trigger_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(trigger_id): Path<i64>,
    Json(request): Json<WorkflowTriggerRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_workflow_run_for_trigger(
        db.as_ref(),
        trigger_id,
        request.parameters,
        request.debug,
    )
    .await
    {
        Ok(run) => {
            emit_workflow_run(&events, run.id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                    run,
                    nodes: Vec::new(),
                })),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_id): Path<i64>,
    Json(request): Json<WorkflowRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_workflow_run(
        db.as_ref(),
        workflow_id,
        request.parameters,
        request.debug,
        request.name,
    )
    .await
    {
        Ok(run) => {
            emit_workflow_run(&events, run.id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                    run,
                    nodes: Vec::new(),
                })),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn claim_workflow_runs_for_scheduler<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(request): Json<SchedulerRunClaimRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let statuses = if request.statuses.is_empty() {
        vec![
            runinator_models::workflows::WorkflowStatus::Queued,
            runinator_models::workflows::WorkflowStatus::Running,
            runinator_models::workflows::WorkflowStatus::DebugPaused,
            runinator_models::workflows::WorkflowStatus::Waiting,
            runinator_models::workflows::WorkflowStatus::ApprovalRequired,
            runinator_models::workflows::WorkflowStatus::Blocked,
        ]
    } else {
        request.statuses
    };
    match repository::claim_workflow_runs_for_scheduler(
        db.as_ref(),
        request.scheduler_id,
        statuses,
        request.lease_until,
        request.limit.unwrap_or(50),
    )
    .await
    {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn claim_ready_nodes<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(request): Json<ReadyNodeClaimRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::claim_ready_nodes(
        db.as_ref(),
        request.scheduler_id,
        request.lease_until,
        request.limit.unwrap_or(50),
    )
    .await
    {
        Ok(nodes) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!(nodes))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn process_ready_node<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(ready_node_id): Path<i64>,
    Json(request): Json<ReadyNodeProcessRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let next_ready = match (
        request.workflow_run_id,
        request.node_id,
        request.next_ready_at,
    ) {
        (Some(workflow_run_id), Some(node_id), Some(next_ready_at)) => {
            Some((workflow_run_id, node_id, next_ready_at))
        }
        _ => None,
    };
    match repository::complete_ready_node(
        db.as_ref(),
        ready_node_id,
        request.scheduler_id,
        next_ready,
    )
    .await
    {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn renew_workflow_run_claim<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<SchedulerRunClaimRenewRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::renew_workflow_run_claim(
        db.as_ref(),
        workflow_run_id,
        request.scheduler_id,
        request.lease_until,
    )
    .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(
                runinator_models::web::TaskResponse {
                    success: true,
                    message: "Workflow run claim renewed".into(),
                },
            )),
        ),
        Ok(false) => not_found(format!("Workflow run claim {workflow_run_id} not held")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn release_workflow_run_claim<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<SchedulerRunClaimReleaseRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::release_workflow_run_claim(db.as_ref(), workflow_run_id, request.scheduler_id)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(
                runinator_models::web::TaskResponse {
                    success: true,
                    message: "Workflow run claim released".into(),
                },
            )),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn cancel_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(broker): Extension<Arc<dyn Broker>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::cancel_workflow_run(db.as_ref(), broker.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn pause_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::pause_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn resume_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::resume_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn replay_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    body: Option<Json<crate::models::WorkflowRunReplayRequest>>,
) -> (StatusCode, Json<ApiResponse>) {
    let from_step_id = body.and_then(|Json(request)| request.from_step_id);
    match repository::replay_workflow_run(db.as_ref(), workflow_run_id, from_step_id).await {
        Ok(run) => {
            emit(&events, AppEvent::WorkflowRunChanged { run_id: run.id });
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                    run,
                    nodes: Vec::new(),
                })),
            )
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn rename_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<crate::models::WorkflowRunRenameRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::set_workflow_run_name(db.as_ref(), workflow_run_id, request.name).await {
        Ok(response) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(response)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

pub(crate) async fn get_workflow_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<WorkflowRunStatusQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(name) = query.name {
        return match repository::fetch_workflow_runs_by_name(
            db.as_ref(),
            name,
            query.open.unwrap_or(false),
        )
        .await
        {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
            Err(err) => api_error(err.to_string()),
        };
    }

    if let Some(workflow_id) = query.workflow_id {
        return match repository::fetch_workflow_runs_for_workflow(db.as_ref(), workflow_id).await {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
            Err(err) => api_error(err.to_string()),
        };
    }

    if let Some(status) = query.status {
        return match repository::fetch_workflow_runs_by_status(db.as_ref(), status).await {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
            Err(err) => api_error(err.to_string()),
        };
    }

    match repository::fetch_recent_workflow_runs(db.as_ref()).await {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<RunStatusQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let Some(status) = query.status else {
        return bad_request("run status query is required");
    };
    match repository::fetch_runs_by_status(db.as_ref(), status).await {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::RunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(run_id): Path<i64>,
    Json(request): Json<RunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_run_status(
        db.as_ref(),
        run_id,
        request.status,
        request.output_json,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit_task_run(&events, run_id, request.status);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_run_chunks<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(run_id): Path<i64>,
    Query(query): Query<ChunkQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_run_chunks(
        db.as_ref(),
        run_id,
        query.cursor,
        query.limit.unwrap_or(100),
    )
    .await
    {
        Ok(chunks) => (StatusCode::OK, Json(ApiResponse::RunChunks(chunks))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn append_run_chunk<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(run_id): Path<i64>,
    Json(chunk): Json<NewRunChunk>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::append_run_chunk(db.as_ref(), run_id, &chunk).await {
        Ok(chunk) => {
            emit(&events, AppEvent::RunChunkAdded { run_id });
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::RunChunks(vec![chunk])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<WorkflowRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_workflow_run_status(
        db.as_ref(),
        workflow_run_id,
        request.status,
        request.active_node_id,
        request.state,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(Some((run, nodes))) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                run,
                nodes,
            })),
        ),
        Ok(None) => not_found(format!("Workflow run {workflow_run_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) fn compute_stale_seconds(updated_at: &str) -> Option<i64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(updated_at).ok()?;
    let now = chrono::Utc::now();
    Some((now - parsed.with_timezone(&chrono::Utc)).num_seconds())
}
