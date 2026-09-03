use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    http::{Method, Request, StatusCode},
    middleware::from_fn_with_state,
    routing::{get, post},
};
use tower::ServiceExt;

use super::{
    CircuitBreakerConfig, CircuitBreakerRejection, CircuitBreakers, circuit_breaker_middleware,
};

fn test_breakers() -> Arc<CircuitBreakers> {
    Arc::new(CircuitBreakers::new(CircuitBreakerConfig {
        failure_rate_threshold: 0.5,
        minimum_number_of_calls: 2,
        // The upstream library evaluates a count window once it is full, so tests use a compact
        // two-call sample while production keeps its conservative one-hundred-call history.
        sliding_window_size: 2,
        cooldown: Duration::from_millis(1),
        ..CircuitBreakerConfig::default()
    }))
}

#[tokio::test]
async fn opens_one_route_family_without_shedding_another() {
    let read_calls = Arc::new(AtomicUsize::new(0));
    let read_handler_calls = read_calls.clone();
    let app = Router::new()
        .route(
            "/read",
            get(move || {
                let calls = read_handler_calls.clone();
                async move {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        StatusCode::REQUEST_TIMEOUT
                    } else {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                }
            }),
        )
        .route("/write", post(|| async { StatusCode::OK }))
        .layer(from_fn_with_state(
            test_breakers(),
            circuit_breaker_middleware,
        ));

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::REQUEST_TIMEOUT);
    let second = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        response
            .extensions()
            .get::<CircuitBreakerRejection>()
            .is_some()
    );
    assert_eq!(read_calls.load(Ordering::SeqCst), 2);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/write")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn client_errors_do_not_contribute_to_a_circuit() {
    let app = Router::new()
        .route(
            "/read",
            get(|| async { StatusCode::BAD_REQUEST })
                .post(|| async { StatusCode::TOO_MANY_REQUESTS }),
        )
        .layer(from_fn_with_state(
            test_breakers(),
            circuit_breaker_middleware,
        ));

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/read")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/read")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}

#[tokio::test]
async fn health_bypasses_a_tripped_read_circuit() {
    let app = Router::new()
        .route("/read", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
        .route("/health", get(|| async { StatusCode::OK }))
        .layer(from_fn_with_state(
            test_breakers(),
            circuit_breaker_middleware,
        ));

    for _ in 0..2 {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/read")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn external_ingress_has_an_independent_circuit() {
    let app = Router::new()
        .route(
            "/webhooks/orchestration/github",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .route("/read", get(|| async { StatusCode::OK }))
        .layer(from_fn_with_state(
            test_breakers(),
            circuit_breaker_middleware,
        ));

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/webhooks/orchestration/github")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
    let rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/webhooks/orchestration/github")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);

    let read = app
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
}

#[tokio::test]
async fn write_control_has_an_independent_circuit() {
    let app = Router::new()
        .route(
            "/write",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
        .route("/read", get(|| async { StatusCode::OK }))
        .layer(from_fn_with_state(
            test_breakers(),
            circuit_breaker_middleware,
        ));

    for _ in 0..2 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/write")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
    let rejected = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/write")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    let read = app
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
}

#[tokio::test]
async fn one_half_open_probe_closes_or_reopens_the_circuit() {
    let failing = Arc::new(AtomicBool::new(true));
    let handler_failing = failing.clone();
    let app = Router::new()
        .route(
            "/read",
            get(move || {
                let failing = handler_failing.clone();
                async move {
                    if failing.load(Ordering::SeqCst) {
                        StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        )
        .layer(from_fn_with_state(
            test_breakers(),
            circuit_breaker_middleware,
        ));

    for _ in 0..2 {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/read")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(5)).await;
    failing.store(false, Ordering::SeqCst);
    let recovered = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(recovered.status(), StatusCode::OK);
    let closed = app
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(closed.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_failed_half_open_probe_reopens_the_circuit() {
    let app = Router::new()
        .route("/read", get(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
        .layer(from_fn_with_state(
            test_breakers(),
            circuit_breaker_middleware,
        ));
    for _ in 0..2 {
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/read")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(5)).await;
    let probe = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(probe.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let rejected = app
        .oneshot(
            Request::builder()
                .uri("/read")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
}
