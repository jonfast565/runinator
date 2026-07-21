use std::time::Duration;

use axum::{Router, body::Body, http::Request, http::StatusCode, routing::get};
use tower::ServiceExt;

use super::{OverloadConfig, apply_overload_protection};

fn enabled_config(timeout: Duration, max_concurrent: usize) -> OverloadConfig {
    OverloadConfig {
        enabled: true,
        max_concurrent_requests: max_concurrent,
        request_timeout: timeout,
    }
}

// a handler slower than the request-timeout budget is aborted with 408.
#[tokio::test]
async fn slow_handler_times_out() {
    let router = Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "done"
        }),
    );
    let router = apply_overload_protection(router, enabled_config(Duration::from_millis(50), 8));
    let response = router
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
}

// a fast handler still succeeds while the layers are engaged.
#[tokio::test]
async fn fast_handler_passes_through_enabled() {
    let router = Router::new().route("/ok", get(|| async { "ok" }));
    let router = apply_overload_protection(router, enabled_config(Duration::from_secs(30), 8));
    let response = router
        .oneshot(Request::builder().uri("/ok").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// a disabled config adds no layers, so even a handler slower than the (ignored) timeout succeeds.
#[tokio::test]
async fn disabled_config_is_passthrough() {
    let router = Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            "done"
        }),
    );
    let config = OverloadConfig {
        enabled: false,
        ..enabled_config(Duration::from_millis(1), 8)
    };
    let router = apply_overload_protection(router, config);
    let response = router
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
