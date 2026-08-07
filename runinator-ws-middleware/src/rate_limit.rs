//! per-principal / per-ip token-bucket rate limiting for the http api.
//!
//! the limiter runs after the auth middleware so it can key by the resolved principal when present
//! and fall back to the connection ip for anonymous/public requests. buckets live in process memory;
//! each replica limits independently, which is the intended behavior for a horizontally scaled api.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use runinator_models::auth::AuthContext;

// prune the bucket map when it grows past this many keys to bound memory under ip churn.
const PRUNE_THRESHOLD: usize = 10_000;

/// runtime configuration for the token-bucket limiter.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitConfig {
    pub enabled: bool,
    /// sustained requests allowed per second (the bucket refill rate).
    pub requests_per_second: f64,
    /// maximum burst capacity (the bucket size).
    pub burst: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            requests_per_second: 50.0,
            burst: 100.0,
        }
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

/// shared, in-memory token-bucket limiter keyed by an opaque principal/ip string.
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// try to spend one token for `key`. returns `Ok(())` when allowed, or `Err(retry_after_secs)`
    /// with the wait before a token is available.
    fn check(&self, key: &str) -> Result<(), f64> {
        let rate = self.config.requests_per_second.max(f64::MIN_POSITIVE);
        let burst = self.config.burst.max(1.0);
        let now = Instant::now();
        let mut buckets = match self.buckets.lock() {
            Ok(guard) => guard,
            // a poisoned lock should not take the api down; fail open.
            Err(poisoned) => poisoned.into_inner(),
        };
        if buckets.len() > PRUNE_THRESHOLD {
            // drop full, idle buckets; they carry no state worth keeping.
            buckets.retain(|_, bucket| {
                let refilled = bucket.tokens + now.duration_since(bucket.last).as_secs_f64() * rate;
                refilled < burst
            });
        }
        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            tokens: burst,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * rate).min(burst);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            return Ok(());
        }
        Err((1.0 - bucket.tokens) / rate)
    }
}

/// strict, always-on throttle for the unauthenticated auth endpoints, keyed by client ip. it runs
/// independently of the configurable global limiter so credential brute force stays bounded even
/// when general rate limiting is disabled. the slow refill with a small burst tolerates a few
/// legitimate retries while making online password guessing impractical.
fn login_throttle() -> &'static RateLimiter {
    static THROTTLE: OnceLock<RateLimiter> = OnceLock::new();
    THROTTLE.get_or_init(|| {
        RateLimiter::new(RateLimitConfig {
            enabled: true,
            // ~1 sustained attempt every 5 seconds.
            requests_per_second: 0.2,
            // absorb a short burst of honest retries before throttling kicks in.
            burst: 10.0,
        })
    })
}

/// spend one login attempt for `ip`. returns `Err(retry_after_secs)` when the bucket is empty.
pub fn check_login_attempt(ip: IpAddr) -> Result<(), f64> {
    login_throttle().check(&format!("login:{ip}"))
}

/// paths exempt from rate limiting so health/metrics scrapers are never throttled.
fn is_exempt(path: &str) -> bool {
    matches!(path, "/health" | "/ready" | "/metrics")
}

/// derive the rate-limit key: the authenticated principal when present, else the connection ip.
fn rate_limit_key(req: &Request<Body>) -> String {
    if let Some(context) = req.extensions().get::<AuthContext>() {
        if let Some(id) = context.principal_id {
            return format!("principal:{id}");
        }
    }
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return format!("ip:{}", addr.ip());
    }
    "anonymous".to_string()
}

/// gate every non-exempt request through the token bucket; reply `429` with `Retry-After` when the
/// bucket is empty.
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !limiter.config.enabled || is_exempt(req.uri().path()) {
        return next.run(req).await;
    }
    let key = rate_limit_key(&req);
    match limiter.check(&key) {
        Ok(()) => next.run(req).await,
        Err(retry_after) => {
            let secs = retry_after.ceil().max(1.0) as u64;
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("Retry-After", secs.to_string())],
                "rate limit exceeded",
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
