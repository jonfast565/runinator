use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::runs::{NewRunArtifact, NewRunChunk};

use crate::events::{EventSender, emit_workflow_node_run, emit_workflow_run};
use crate::handlers::runs::ChunkQuery;
use crate::models::{
    ApiResponse, WorkflowNodeRunExecutorClaimRequest, WorkflowNodeRunExecutorReleaseRequest,
    WorkflowNodeRunInputRequest, WorkflowNodeRunRequest, WorkflowNodeRunStatusRequest,
};
use crate::repository;
use crate::responses::api_error;

pub(crate) async fn create_workflow_node_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_run_workflow(
        db.as_ref(),
        &ctx,
        workflow_run_id,
        runinator_models::auth::Permission::Run,
    )
    .await
    {
        return reply;
    }
    match repository::create_workflow_node_run(
        db.as_ref(),
        workflow_run_id,
        request.node_id,
        request.parameters,
    )
    .await
    {
        Ok(step) => {
            emit_workflow_run(&events, workflow_run_id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowNodeRun(step)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_workflow_node_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::update_workflow_node_run(
        db.as_ref(),
        node_run_id,
        request.status,
        request.attempt,
        request.parameters,
        request.output_json,
        request.state,
        request.transition_reason,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn resolve_workflow_input<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunInputRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_node_run_workflow(
        db.as_ref(),
        &ctx,
        node_run_id,
        runinator_models::auth::Permission::Run,
    )
    .await
    {
        return reply;
    }
    match repository::resolve_workflow_input(
        db.as_ref(),
        node_run_id,
        request.output_json,
        request.resolved_by,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn claim_workflow_node_run_executor<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunExecutorClaimRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::claim_workflow_node_run_executor(
        db.as_ref(),
        node_run_id,
        request.replica_id,
        request.claimed_at,
    )
    .await
    {
        Ok(resp) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn release_workflow_node_run_executor<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunExecutorReleaseRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::release_workflow_node_run_executor(
        db.as_ref(),
        node_run_id,
        request.replica_id,
        request.released_at,
    )
    .await
    {
        Ok(resp) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow_node_run_chunks<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Query(query): Query<ChunkQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_node_run_workflow(
        db.as_ref(),
        &ctx,
        node_run_id,
        runinator_models::auth::Permission::View,
    )
    .await
    {
        return reply;
    }
    match repository::fetch_workflow_node_run_chunks(
        db.as_ref(),
        node_run_id,
        query.cursor,
        query.limit.unwrap_or(100),
    )
    .await
    {
        Ok(chunks) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowNodeRunChunks(chunks)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn append_workflow_node_run_chunk<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(chunk): Json<NewRunChunk>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::append_workflow_node_run_chunk(db.as_ref(), node_run_id, &chunk).await {
        Ok(chunk) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowNodeRunChunks(vec![chunk])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow_node_run_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_node_run_workflow(
        db.as_ref(),
        &ctx,
        node_run_id,
        runinator_models::auth::Permission::View,
    )
    .await
    {
        return reply;
    }
    match repository::fetch_workflow_node_run_artifacts(db.as_ref(), node_run_id).await {
        Ok(artifacts) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowNodeRunArtifacts(artifacts)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow_run_deliverables<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_run_workflow(
        db.as_ref(),
        &ctx,
        workflow_run_id,
        runinator_models::auth::Permission::View,
    )
    .await
    {
        return reply;
    }
    match repository::fetch_workflow_run_deliverables(db.as_ref(), workflow_run_id).await {
        Ok(deliverables) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRunDeliverables(deliverables)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn add_workflow_node_run_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(artifact): Json<NewRunArtifact>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::add_workflow_node_run_artifact(db.as_ref(), node_run_id, &artifact).await {
        Ok(artifact) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowNodeRunArtifacts(vec![artifact])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}
