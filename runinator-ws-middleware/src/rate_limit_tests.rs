use super::*;

use axum::{
    Router,
    http::{Request, StatusCode, header},
    middleware::from_fn_with_state,
    routing::get,
};
use tower::ServiceExt;
use uuid::Uuid;

#[test]
fn bucket_allows_burst_then_blocks() {
    let limiter = RateLimiter::new(RateLimitConfig {
        enabled: true,
        requests_per_second: 1.0,
        burst: 3.0,
    });
    // three immediate requests fit the burst.
    assert!(limiter.check("k").is_ok());
    assert!(limiter.check("k").is_ok());
    assert!(limiter.check("k").is_ok());
    // the fourth is rejected with a positive retry-after.
    let retry = limiter.check("k").unwrap_err();
    assert!(retry > 0.0);
}

#[test]
fn keys_are_isolated() {
    let limiter = RateLimiter::new(RateLimitConfig {
        enabled: true,
        requests_per_second: 1.0,
        burst: 1.0,
    });
    assert!(limiter.check("a").is_ok());
    // a separate key has its own bucket.
    assert!(limiter.check("b").is_ok());
    assert!(limiter.check("a").is_err());
}

#[test]
fn quota_rounds_a_fractional_burst_up_to_the_next_whole_permit() {
    let limiter = RateLimiter::new(RateLimitConfig {
        enabled: true,
        requests_per_second: 1.0,
        burst: 1.1,
    });
    assert!(limiter.check("key").is_ok());
    assert!(limiter.check("key").is_ok());
    assert!(limiter.check("key").is_err());
}

#[test]
fn authenticated_principals_take_precedence_over_connection_ip() {
    let principal_id = Uuid::new_v4();
    let mut request = Request::new(Body::empty());
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 10], 443))));
    let mut context = AuthContext::disabled_platform_admin();
    context.principal_id = Some(principal_id);
    request.extensions_mut().insert(context);
    assert_eq!(
        rate_limit_key(&request),
        format!("principal:{principal_id}")
    );
}

#[test]
fn anonymous_requests_fall_back_to_the_connection_ip() {
    let mut request = Request::new(Body::empty());
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 11], 443))));
    assert_eq!(rate_limit_key(&request), "ip:203.0.113.11");
}

#[test]
fn health_and_metrics_paths_are_exempt() {
    for path in ["/health", "/ready", "/metrics"] {
        assert!(is_exempt(path));
    }
    assert!(!is_exempt("/workflows"));
}

#[test]
fn stale_key_maintenance_starts_after_the_existing_threshold() {
    assert!(!needs_maintenance(PRUNE_THRESHOLD));
    assert!(needs_maintenance(PRUNE_THRESHOLD + 1));
}

#[tokio::test]
async fn rejected_requests_expose_a_positive_retry_after_header() {
    let limiter = Arc::new(RateLimiter::new(RateLimitConfig {
        enabled: true,
        requests_per_second: 1.0,
        burst: 1.0,
    }));
    let app = Router::new()
        .route("/limited", get(|| async { StatusCode::OK }))
        .layer(from_fn_with_state(limiter, rate_limit_middleware));

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/limited")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .oneshot(
            Request::builder()
                .uri("/limited")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(second.headers()[header::RETRY_AFTER], "1");
}
