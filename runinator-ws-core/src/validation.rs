//! HTTP extractors that make type-owned validation mandatory at JSON ingress.

use axum::{
    Json,
    extract::{FromRequest, OptionalFromRequest, Request},
    http::StatusCode,
};
use runinator_models::validation::Validate;
use serde::de::DeserializeOwned;

use crate::models::{ApiError, ApiResponse};

/// A JSON body that has both deserialized successfully and passed its type's [`Validate`] rules.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = (StatusCode, Json<ApiResponse>);

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = <Json<T> as FromRequest<S>>::from_request(request, state)
            .await
            .map_err(|rejection| rejection_reply(rejection.body_text()))?;
        value.validate().map_err(validation_reply)?;
        Ok(Self(value))
    }
}

impl<S, T> OptionalFromRequest<S> for ValidatedJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = (StatusCode, Json<ApiResponse>);

    async fn from_request(request: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        let value = <Json<T> as OptionalFromRequest<S>>::from_request(request, state)
            .await
            .map_err(|rejection| rejection_reply(rejection.body_text()))?;
        let Some(Json(value)) = value else {
            return Ok(None);
        };
        value.validate().map_err(validation_reply)?;
        Ok(Some(Self(value)))
    }
}

fn validation_reply(
    error: runinator_models::validation::ValidationError,
) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::ApiError(ApiError {
            message: error.message,
            path: Some(error.path),
            expected: None,
            actual: None,
        })),
    )
}

fn rejection_reply(message: String) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}

#[cfg(test)]
mod tests {
    use axum::{Router, body::Body, http::Request, routing::post};
    use runinator_models::auth::LoginRequest;
    use tower::ServiceExt;

    use super::*;

    async fn login(ValidatedJson(_): ValidatedJson<LoginRequest>) -> StatusCode {
        StatusCode::NO_CONTENT
    }

    #[tokio::test]
    async fn rejects_semantically_invalid_json_before_the_handler() {
        let app = Router::new().route("/", post(login));
        let response = app
            .oneshot(
                Request::post("/")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"username":" ","password":"secret"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
