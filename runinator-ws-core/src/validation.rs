//! HTTP extractors that make type-owned validation mandatory at JSON ingress.

use axum::{
    Json,
    extract::{FromRequest, OptionalFromRequest, Request},
    http::StatusCode,
};
use runinator_models::{
    validation::{Validate, dynamic_value},
    value::Value,
};
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
        let Json(raw) = <Json<serde_json::Value> as FromRequest<S>>::from_request(request, state)
            .await
            .map_err(|rejection| rejection_reply(rejection.body_text()))?;
        dynamic_value("payload", &Value::from(raw.clone())).map_err(validation_reply)?;
        let value: T =
            serde_json::from_value(raw).map_err(|error| rejection_reply(error.to_string()))?;
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
        let value =
            <Json<serde_json::Value> as OptionalFromRequest<S>>::from_request(request, state)
                .await
                .map_err(|rejection| rejection_reply(rejection.body_text()))?;
        let Some(Json(raw)) = value else {
            return Ok(None);
        };
        dynamic_value("payload", &Value::from(raw.clone())).map_err(validation_reply)?;
        let value: T =
            serde_json::from_value(raw).map_err(|error| rejection_reply(error.to_string()))?;
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
    use std::{fs, path::Path};

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

    #[test]
    fn handler_crates_do_not_accept_unvalidated_json_bodies() {
        fn inspect(path: &Path, violations: &mut Vec<String>) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    inspect(&path, violations);
                } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                    let source = fs::read_to_string(&path).unwrap();
                    for (line_number, line) in source.lines().enumerate() {
                        if line.contains(": Json<") {
                            violations.push(format!(
                                "{}:{} uses raw Json request extraction",
                                path.display(),
                                line_number + 1
                            ));
                        }
                    }
                }
            }
        }

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut violations = Vec::new();
        for crate_name in [
            "runinator-ws-identity",
            "runinator-ws-authoring",
            "runinator-ws-runtime",
        ] {
            inspect(
                &workspace.join(crate_name).join("src").join("handlers"),
                &mut violations,
            );
        }
        assert!(
            violations.is_empty(),
            "all JSON request bodies must use ValidatedJson:\n{}",
            violations.join("\n")
        );
    }
}
