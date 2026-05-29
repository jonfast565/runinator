use axum::{Json, http::StatusCode};
use runinator_models::web::TaskResponse;
use runinator_workflows::WorkflowValidationError;

use crate::models::{ApiError, ApiResponse};

pub(crate) fn api_error(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    api_error_status(StatusCode::INTERNAL_SERVER_ERROR, ApiError::new(message))
}

pub(crate) fn validation_error(
    err: &(dyn std::error::Error + 'static),
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(workflow_err) = err.downcast_ref::<WorkflowValidationError>()
        && let Some(diagnostic) = workflow_err.type_diagnostic()
    {
        return api_error_status(
            StatusCode::BAD_REQUEST,
            ApiError {
                message: diagnostic.message.clone(),
                path: Some(diagnostic.path.clone()),
                expected: Some(diagnostic.expected.clone()),
                actual: Some(diagnostic.actual.clone()),
            },
        );
    }
    api_error_status(StatusCode::BAD_REQUEST, ApiError::new(err.to_string()))
}

fn api_error_status(status: StatusCode, error: ApiError) -> (StatusCode, Json<ApiResponse>) {
    (status, Json(ApiResponse::ApiError(error)))
}

pub(crate) fn not_found(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}

pub(crate) fn bad_request(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}

pub(crate) fn task_response_success(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::OK,
        Json(ApiResponse::TaskResponse(TaskResponse {
            success: true,
            message: message.into(),
        })),
    )
}
