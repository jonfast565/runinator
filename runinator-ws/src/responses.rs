use axum::{Json, http::StatusCode};
use runinator_models::web::TaskResponse;

use crate::models::{ApiError, ApiResponse};

pub(crate) fn api_error(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::ApiError(ApiError {
            message: message.into(),
        })),
    )
}

pub(crate) fn not_found(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::ApiError(ApiError {
            message: message.into(),
        })),
    )
}

pub(crate) fn bad_request(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::ApiError(ApiError {
            message: message.into(),
        })),
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
