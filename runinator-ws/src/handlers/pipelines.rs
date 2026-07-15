use std::sync::Arc;
use uuid::Uuid;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, Permission},
    pipelines::Pipeline,
};

use crate::authz;
use crate::events::{AppEvent, EventSender, emit};
use crate::models::{ApiResponse, PipelineOwnerRequest};
use crate::repository;
use crate::responses::{api_error, not_found};

pub(crate) async fn get_pipelines<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_pipelines(db.as_ref()).await {
        Ok(pipelines) => {
            // scope to the caller's org first (cross-tenant pipelines are never listed), then to the
            // grant-based visibility set (None = admin/auth-disabled = all).
            let pipelines: Vec<_> = pipelines
                .into_iter()
                .filter(|pipeline| authz::org_visible(&ctx, pipeline.org_id))
                .collect();
            let visible = authz::visible_pipeline_ids(db.as_ref(), &ctx).await;
            let pipelines = match visible {
                Some(ids) => pipelines
                    .into_iter()
                    .filter(|pipeline| pipeline.id.is_some_and(|id| ids.contains(&id)))
                    .collect(),
                None => pipelines,
            };
            (StatusCode::OK, Json(ApiResponse::PipelineList(pipelines)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_pipeline<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline(db.as_ref(), &ctx, pipeline_id, Permission::View).await
    {
        return reply;
    }
    match repository::fetch_pipeline(db.as_ref(), pipeline_id).await {
        // a cross-tenant pipeline is not-found even if a stray grant would otherwise reveal it.
        Ok(Some(pipeline)) if !authz::org_visible(&ctx, pipeline.org_id) => {
            not_found(format!("Pipeline {pipeline_id} not found"))
        }
        Ok(Some(pipeline)) => (StatusCode::OK, Json(ApiResponse::Pipeline(pipeline))),
        Ok(None) => not_found(format!("Pipeline {pipeline_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_pipeline<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(mut pipeline): Json<Pipeline>,
) -> (StatusCode, Json<ApiResponse>) {
    // a create always mints a fresh id and is owned by the creator's active org (None = global).
    pipeline.id = None;
    pipeline.org_id = ctx.org_id;
    match repository::upsert_pipeline(db.as_ref(), &pipeline).await {
        Ok(pipeline) => {
            if let Some(id) = pipeline.id {
                authz::grant_pipeline_owner(db.as_ref(), &ctx, id).await;
            }
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::Pipeline(pipeline)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_pipeline<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    Json(mut pipeline): Json<Pipeline>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline(db.as_ref(), &ctx, pipeline_id, Permission::Edit).await
    {
        return reply;
    }
    pipeline.id = Some(pipeline_id);
    // preserve the stored org on update so a client cannot re-tenant a pipeline by editing it.
    pipeline.org_id = match repository::fetch_pipeline(db.as_ref(), pipeline_id).await {
        Ok(Some(existing)) => existing.org_id,
        Ok(None) => return not_found(format!("Pipeline {pipeline_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    match repository::upsert_pipeline(db.as_ref(), &pipeline).await {
        Ok(pipeline) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::Pipeline(pipeline)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// reassign a pipeline's owning organization. requires `Own` on the pipeline (owner or platform
/// admin); moving it into an org additionally requires org-admin on the target org.
pub(crate) async fn set_pipeline_owner<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    Json(request): Json<PipelineOwnerRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline(db.as_ref(), &ctx, pipeline_id, Permission::Own).await
    {
        return reply;
    }
    if let Some(org_id) = request.org_id {
        if let Err(reply) = authz::require_org_admin(&ctx, org_id) {
            return reply;
        }
    }
    match repository::set_pipeline_org(db.as_ref(), pipeline_id, request.org_id).await {
        Ok(()) => {
            emit(&events, AppEvent::WorkflowsChanged);
            match repository::fetch_pipeline(db.as_ref(), pipeline_id).await {
                Ok(Some(pipeline)) => (StatusCode::OK, Json(ApiResponse::Pipeline(pipeline))),
                Ok(None) => not_found(format!("Pipeline {pipeline_id} not found")),
                Err(err) => api_error(err.to_string()),
            }
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_pipeline<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline(db.as_ref(), &ctx, pipeline_id, Permission::Edit).await
    {
        return reply;
    }
    match repository::delete_pipeline(db.as_ref(), pipeline_id).await {
        Ok(resp) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}
