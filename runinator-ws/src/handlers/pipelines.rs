use std::sync::Arc;
use uuid::Uuid;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, Permission},
    pipelines::{Pipeline, PipelineTrigger},
};

use crate::authz;
use crate::events::{
    AppEvent, AppEventKind, EventSender, emit, emit_pipeline_run, emit_workflows_changed,
    nudge_wake_publisher,
};
use crate::models::{ApiResponse, PipelineOwnerRequest, PipelineRunRequest};
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
            emit_workflows_changed(&events, pipeline.org_id);
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
            emit_workflows_changed(&events, pipeline.org_id);
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
            emit_workflows_changed(&events, request.org_id);
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
    let org_id = match repository::fetch_pipeline(db.as_ref(), pipeline_id).await {
        Ok(Some(pipeline)) => pipeline.org_id.or(ctx.org_id),
        Ok(None) => ctx.org_id,
        Err(_) => ctx.org_id,
    };
    match repository::delete_pipeline(db.as_ref(), pipeline_id).await {
        Ok(resp) => {
            emit_workflows_changed(&events, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

// --- pipeline triggers ---

pub(crate) async fn get_pipeline_triggers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline(db.as_ref(), &ctx, pipeline_id, Permission::View).await
    {
        return reply;
    }
    match repository::fetch_pipeline_triggers(db.as_ref(), pipeline_id).await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::PipelineTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn upsert_pipeline_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    Json(mut trigger): Json<PipelineTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline(db.as_ref(), &ctx, pipeline_id, Permission::Edit).await
    {
        return reply;
    }
    trigger.pipeline_id = pipeline_id;
    match repository::upsert_pipeline_trigger(db.as_ref(), &trigger).await {
        Ok(trigger) => {
            let org_id = pipeline_org(db.as_ref(), pipeline_id, ctx.org_id).await;
            emit_workflows_changed(&events, org_id);
            (StatusCode::OK, Json(ApiResponse::PipelineTrigger(trigger)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn update_pipeline_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    Json(mut trigger): Json<PipelineTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline_trigger(db.as_ref(), &ctx, trigger_id, Permission::Edit).await
    {
        return reply;
    }
    trigger.id = Some(trigger_id);
    match repository::upsert_pipeline_trigger(db.as_ref(), &trigger).await {
        Ok(trigger) => {
            let org_id = pipeline_org(db.as_ref(), trigger.pipeline_id, ctx.org_id).await;
            emit_workflows_changed(&events, org_id);
            (StatusCode::OK, Json(ApiResponse::PipelineTrigger(trigger)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_pipeline_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline_trigger(db.as_ref(), &ctx, trigger_id, Permission::Edit).await
    {
        return reply;
    }
    let org_id = match repository::fetch_pipeline_trigger(db.as_ref(), trigger_id).await {
        Ok(Some(trigger)) => pipeline_org(db.as_ref(), trigger.pipeline_id, ctx.org_id).await,
        _ => ctx.org_id,
    };
    match repository::delete_pipeline_trigger(db.as_ref(), trigger_id).await {
        Ok(resp) => {
            emit_workflows_changed(&events, org_id);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

// --- pipeline runs ---

pub(crate) async fn create_pipeline_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_id): Path<Uuid>,
    Json(request): Json<PipelineRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline(db.as_ref(), &ctx, pipeline_id, Permission::Run).await
    {
        return reply;
    }
    match repository::create_manual_pipeline_run(
        db.as_ref(),
        pipeline_id,
        request.parameters,
        None,
        Some("api".into()),
    )
    .await
    {
        Ok(run) => {
            let org_id = repository::org_id_for_pipeline_run(db.as_ref(), run.id).await;
            emit_pipeline_run(&events, run.id, org_id);
            emit(
                &events,
                AppEvent::new(org_id, AppEventKind::PipelineRunActivity),
            );
            nudge_wake_publisher(&events);
            (StatusCode::ACCEPTED, Json(ApiResponse::PipelineRun(run)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn create_pipeline_trigger_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    Json(request): Json<PipelineRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline_trigger(db.as_ref(), &ctx, trigger_id, Permission::Run).await
    {
        return reply;
    }
    match repository::create_pipeline_run_for_trigger(
        db.as_ref(),
        trigger_id,
        request.parameters,
        None,
        Some("api".into()),
    )
    .await
    {
        Ok(run) => {
            let org_id = repository::org_id_for_pipeline_run(db.as_ref(), run.id).await;
            emit_pipeline_run(&events, run.id, org_id);
            emit(
                &events,
                AppEvent::new(org_id, AppEventKind::PipelineRunActivity),
            );
            nudge_wake_publisher(&events);
            (StatusCode::ACCEPTED, Json(ApiResponse::PipelineRun(run)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_pipeline_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_recent_pipeline_runs(db.as_ref(), 200).await {
        Ok(runs) => {
            let visible = authz::visible_pipeline_ids(db.as_ref(), &ctx).await;
            let runs = match visible {
                Some(ids) => runs
                    .into_iter()
                    .filter(|run| ids.contains(&run.pipeline_id))
                    .collect(),
                None => runs,
            };
            (StatusCode::OK, Json(ApiResponse::PipelineRunList(runs)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_pipeline_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline_run(db.as_ref(), &ctx, pipeline_run_id, Permission::View).await
    {
        return reply;
    }
    match repository::fetch_pipeline_run_detail(db.as_ref(), pipeline_run_id).await {
        Ok(Some(detail)) => (StatusCode::OK, Json(ApiResponse::PipelineRunDetail(detail))),
        Ok(None) => not_found(format!("Pipeline run {pipeline_run_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn cancel_pipeline_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(pipeline_run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) =
        authz::require_pipeline_run(db.as_ref(), &ctx, pipeline_run_id, Permission::Run).await
    {
        return reply;
    }
    match repository::cancel_pipeline_run(db.as_ref(), pipeline_run_id).await {
        Ok(resp) => {
            let org_id = repository::org_id_for_pipeline_run(db.as_ref(), pipeline_run_id).await;
            emit_pipeline_run(&events, pipeline_run_id, org_id);
            emit(
                &events,
                AppEvent::new(org_id, AppEventKind::PipelineRunActivity),
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn pipeline_org<T: DatabaseImpl>(
    db: &T,
    pipeline_id: Uuid,
    fallback: Option<Uuid>,
) -> Option<Uuid> {
    match repository::fetch_pipeline(db, pipeline_id).await {
        Ok(Some(pipeline)) => pipeline.org_id.or(fallback),
        _ => fallback,
    }
}
