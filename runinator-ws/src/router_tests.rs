use axum::body::Body;
use axum::body::to_bytes;
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::{Router, routing::get};
use tower::ServiceExt;

use super::{CorsConfig, cors_layer, handle_panic};

// the various payload types `panic!`/`assert!` produce should all map to a 500 without the panic
// handler itself panicking on an unexpected payload type, and the body must be the generic envelope
// so panic internals never reach the client.
#[tokio::test]
async fn handle_panic_returns_internal_error_envelope() {
    for payload in [
        Box::new("boom") as Box<dyn std::any::Any + Send>,
        Box::new(String::from("boom")),
        Box::new(42u32),
    ] {
        let response = handle_panic(payload);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["message"], "internal server error");
    }
}

#[test]
fn cors_config_rejects_wildcards_and_paths() {
    assert!(CorsConfig::new(vec!["*".into()]).is_err());
    assert!(CorsConfig::new(vec!["https://example.com/api".into()]).is_err());
    assert!(CorsConfig::new(vec!["https://example.com".into()]).is_ok());
}

#[tokio::test]
async fn cors_only_reflects_an_explicitly_allowed_origin() {
    let config = CorsConfig::new(vec!["https://command.example".into()]).unwrap();
    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(cors_layer(&config));

    let allowed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::ORIGIN, "https://command.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        allowed.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(&HeaderValue::from_static("https://command.example"))
    );

    let rejected = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header(header::ORIGIN, "https://attacker.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        rejected
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}
