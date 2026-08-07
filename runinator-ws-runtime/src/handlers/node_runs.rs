use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::runs::{NewRunArtifact, NewRunChunk};

use crate::handlers::runs::ChunkQuery;
use crate::repository;
use runinator_ws_core::events::{EventSender, emit_workflow_node_run, emit_workflow_run};
use runinator_ws_core::models::{
    ApiResponse, WorkflowNodeRunExecutorClaimRequest, WorkflowNodeRunExecutorReleaseRequest,
    WorkflowNodeRunInputRequest, WorkflowNodeRunRequest, WorkflowNodeRunStatusRequest,
};
use runinator_ws_core::openapi::docs::{CURSOR, EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::api_error;

pub async fn create_workflow_node_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_run_workflow(
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
        request.prev_node_run_id,
    )
    .await
    {
        Ok(step) => {
            let org_id = repository::org_id_for_workflow_run(db.as_ref(), workflow_run_id).await;
            emit_workflow_run(&events, workflow_run_id, org_id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowNodeRun(step)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_workflow_node_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_service_or_admin(&ctx) {
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

pub async fn resolve_workflow_input<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunInputRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_node_run_workflow(
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

pub async fn claim_workflow_node_run_executor<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunExecutorClaimRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    match repository::claim_workflow_node_run_executor(
        db.as_ref(),
        node_run_id,
        request.replica_id,
        request.claimed_at,
        request.stale_before.unwrap_or(request.claimed_at),
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

pub async fn release_workflow_node_run_executor<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(request): Json<WorkflowNodeRunExecutorReleaseRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_service_or_admin(&ctx) {
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

pub async fn get_workflow_node_run_chunks<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Query(query): Query<ChunkQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_node_run_workflow(
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

pub async fn append_workflow_node_run_chunk<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(chunk): Json<NewRunChunk>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_service_or_admin(&ctx) {
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

pub async fn get_workflow_node_run_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_node_run_workflow(
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

pub async fn get_workflow_run_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_run_workflow(
        db.as_ref(),
        &ctx,
        workflow_run_id,
        runinator_models::auth::Permission::View,
    )
    .await
    {
        return reply;
    }
    match repository::fetch_workflow_run_artifacts(db.as_ref(), workflow_run_id).await {
        Ok(artifacts) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRunArtifacts(artifacts)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_workflow_run_transitions<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(workflow_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_run_workflow(
        db.as_ref(),
        &ctx,
        workflow_run_id,
        runinator_models::auth::Permission::View,
    )
    .await
    {
        return reply;
    }
    match repository::fetch_run_transitions(db.as_ref(), workflow_run_id).await {
        Ok(transitions) => (
            StatusCode::OK,
            Json(ApiResponse::NodeTransitions(transitions)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_workflow_node_transitions<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path((workflow_id, node_id)): Path<(Uuid, String)>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_workflow(
        db.as_ref(),
        &ctx,
        workflow_id,
        runinator_models::auth::Permission::View,
    )
    .await
    {
        return reply;
    }
    match repository::fetch_node_transition_stats(db.as_ref(), workflow_id, Some(node_id)).await {
        Ok(stats) => (
            StatusCode::OK,
            Json(ApiResponse::NodeTransitionStats(stats)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn add_workflow_node_run_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<runinator_models::auth::AuthContext>,
    Path(node_run_id): Path<Uuid>,
    Json(artifact): Json<NewRunArtifact>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = runinator_ws_middleware::authz::require_service_or_admin(&ctx) {
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

/// the `node_runs` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, patch, post};
    axum::Router::new()
        .route(
            "/workflow_runs/{id}/nodes",
            post(create_workflow_node_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/claim",
            post(claim_workflow_node_run_executor::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/release",
            post(release_workflow_node_run_executor::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}",
            patch(update_workflow_node_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/input",
            post(resolve_workflow_input::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/chunks",
            get(get_workflow_node_run_chunks::<T>)
                .post(append_workflow_node_run_chunk::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/artifacts",
            get(get_workflow_node_run_artifacts::<T>)
                .post(add_workflow_node_run_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/artifacts",
            get(get_workflow_run_artifacts::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/transitions",
            get(get_workflow_run_transitions::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/nodes/{node_id}/transitions",
            get(get_workflow_node_transitions::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "post",
        "/workflow_runs/{id}/nodes",
        "Control Plane",
        "Create a workflow node run",
        "Service-control endpoint used by the reducer to create a node-run record.",
        false,
        json_body("Node-run creation payload.", Example::NodeRun),
        &[],
        200,
        "workflow node run",
        Example::NodeRun,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/claim",
        "Control Plane",
        "Claim a node run for execution",
        "Worker-control endpoint used to claim a node run before executing the provider action.",
        false,
        json_body("Executor claim payload.", Example::NodeRunClaim),
        &[],
        200,
        "node run claimed",
        Example::NodeRun,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/release",
        "Control Plane",
        "Release a node-run claim",
        "Worker-control endpoint used to release a node-run execution claim.",
        false,
        json_body("Executor release payload.", Example::NodeRunRelease),
        &[],
        200,
        "node-run claim released",
        Example::TaskResponse,
    ),
    endpoint(
        "patch",
        "/workflow_node_runs/{id}",
        "Control Plane",
        "Update a workflow node run",
        "Worker-control endpoint used to update node-run status, attempt, parameters, output, state, or message.",
        false,
        json_body("Node-run status update.", Example::NodeRunStatus),
        &[],
        200,
        "node run updated",
        Example::NodeRun,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/input",
        "Control Plane",
        "Resolve a node-run input",
        "Records a human or external input resolution for a node run waiting on input.",
        false,
        json_body("Resolved input payload.", Example::NodeRunInput),
        &[],
        200,
        "node-run input resolved",
        Example::NodeRun,
    ),
    endpoint(
        "get",
        "/workflow_node_runs/{id}/chunks",
        "Control Plane",
        "List node-run chunks",
        "Returns streamed chunks for a workflow node run.",
        false,
        None,
        CURSOR,
        200,
        "node-run chunks",
        Example::RunChunk,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/chunks",
        "Control Plane",
        "Append a node-run chunk",
        "Appends stdout, stderr, log, or structured chunks for a workflow node run.",
        false,
        json_body("Node-run chunk to append.", Example::RunChunk),
        &[],
        202,
        "node-run chunk appended",
        Example::RunChunk,
    ),
    endpoint(
        "get",
        "/workflow_node_runs/{id}/artifacts",
        "Artifacts",
        "List node-run artifacts",
        "Lists artifacts attached to one workflow node run.",
        false,
        None,
        &[],
        200,
        "node-run artifacts",
        Example::Artifact,
    ),
    endpoint(
        "post",
        "/workflow_node_runs/{id}/artifacts",
        "Artifacts",
        "Attach a node-run artifact",
        "Registers an artifact produced by one workflow node run.",
        false,
        json_body("Artifact metadata to attach.", Example::Artifact),
        &[],
        202,
        "node-run artifact attached",
        Example::Artifact,
    ),
    endpoint(
        "get",
        "/workflow_runs/{id}/artifacts",
        "Workflow Runs",
        "List workflow run artifacts",
        "Lists artifacts declared by output nodes in one workflow run.",
        false,
        None,
        &[],
        200,
        "workflow run artifacts",
        Example::ArtifactList,
    ),
];
