use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use runinator_models::auth::AuthContext;
use runinator_models::capabilities::Capability;
use runinator_models::provisioning::{NodeBackendsResponse, ScaleNodesRequest, StopNodeRequest};
use runinator_provisioner::ProvisionerRegistry;

use crate::models::ApiResponse;
use crate::responses::api_error;

/// list every configured provisioning backend and the node kinds it can manage.
#[utoipa::path(
    get,
    path = "/nodes/backends",
    tag = "Nodes",
    responses((status = 200, description = "provisioning backends", body = serde_json::Value)),
)]
pub(crate) async fn get_node_backends(
    Extension(registry): Extension<Arc<ProvisionerRegistry>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    let backends = registry.backends().await;
    (
        StatusCode::OK,
        Json(ApiResponse::NodeBackends(NodeBackendsResponse { backends })),
    )
}

/// list current node groups (desired/available counts) across every configured backend.
#[utoipa::path(
    get,
    path = "/nodes",
    tag = "Nodes",
    responses((status = 200, description = "provisioned node groups", body = serde_json::Value)),
)]
pub(crate) async fn get_nodes(
    Extension(registry): Extension<Arc<ProvisionerRegistry>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_service_or_admin(&ctx) {
        return reply;
    }
    let groups = registry.list_all().await;
    (StatusCode::OK, Json(ApiResponse::NodeGroupList(groups)))
}

/// set the desired node count for a kind on a backend (spin up or scale down).
pub(crate) async fn scale_nodes(
    Extension(registry): Extension<Arc<ProvisionerRegistry>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<ScaleNodesRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_capability(&ctx, Capability::NodesScale) {
        return reply;
    }
    let provisioner = match registry.require(request.backend) {
        Ok(provisioner) => provisioner,
        Err(err) => return api_error(err.to_string()),
    };
    match provisioner
        .scale(request.kind, request.desired, &request.spec)
        .await
    {
        Ok(group) => (StatusCode::OK, Json(ApiResponse::NodeGroup(group))),
        Err(err) => api_error(err.to_string()),
    }
}

/// stop/remove a single provisioned node instance.
pub(crate) async fn stop_node(
    Extension(registry): Extension<Arc<ProvisionerRegistry>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<StopNodeRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_capability(&ctx, Capability::NodesScale) {
        return reply;
    }
    let provisioner = match registry.require(request.backend) {
        Ok(provisioner) => provisioner,
        Err(err) => return api_error(err.to_string()),
    };
    match provisioner.stop(&request.node_id).await {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "stopped": request.node_id,
            }))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}
