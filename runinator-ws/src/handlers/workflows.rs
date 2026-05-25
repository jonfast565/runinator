use std::sync::Arc;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::StatusCode,
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition};
use serde::Deserialize;

use crate::events::{AppEvent, EventSender, emit};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, not_found, validation_error};

pub(crate) async fn upsert_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Json(workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::upsert_workflow(db.as_ref(), &workflow).await {
        Ok(workflow) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::Workflow(workflow)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn validate_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::validate_workflow_definition_with_catalog(db.as_ref(), &workflow).await {
        Ok(workflow) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Err(err) => validation_error(err.as_ref()),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorkflowQuery {
    pub(crate) name: Option<String>,
}

pub(crate) async fn get_workflows<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<WorkflowQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(name) = query.name {
        return match repository::fetch_workflow_by_name(db.as_ref(), name).await {
            Ok(Some(workflow)) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
            Ok(None) => not_found("Workflow not found"),
            Err(err) => api_error(err.to_string()),
        };
    }

    match repository::fetch_workflows(db.as_ref()).await {
        Ok(workflows) => (StatusCode::OK, Json(ApiResponse::WorkflowList(workflows))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn import_workflow_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Json(bundle): Json<WorkflowBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::import_workflow_bundle(db.as_ref(), bundle).await {
        Ok(bundle) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn export_workflow_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::export_workflow_bundle(db.as_ref(), None).await {
        Ok(bundle) => (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn export_single_workflow_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::export_workflow_bundle(db.as_ref(), Some(workflow_id)).await {
        Ok(bundle) if bundle.workflows.is_empty() => {
            not_found(format!("Workflow {workflow_id} not found"))
        }
        Ok(bundle) => (StatusCode::OK, Json(ApiResponse::WorkflowBundle(bundle))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn get_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow(db.as_ref(), workflow_id).await {
        Ok(Some(workflow)) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Ok(None) => not_found(format!("Workflow {workflow_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn delete_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::delete_workflow(db.as_ref(), workflow_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}
