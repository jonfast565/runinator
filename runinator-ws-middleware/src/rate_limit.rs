//! per-principal / per-ip token-bucket rate limiting for the http API.
//!
//! the limiter runs after the auth middleware so it can key by the resolved principal when present
//! and fall back to the connection ip for anonymous/public requests. buckets live in process memory;
//! each replica limits independently, which is the intended behavior for a horizontally scaled API.

use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use governor::{
    DefaultKeyedRateLimiter, Quota,
    clock::{Clock, DefaultClock},
};
use runinator_models::auth::AuthContext;

// prune the bucket map when it grows past this many keys to bound memory under ip churn.
const PRUNE_THRESHOLD: usize = 10_000;

fn needs_maintenance(key_count: usize) -> bool {
    key_count > PRUNE_THRESHOLD
}

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

impl RateLimitConfig {
    /// Validate the user-facing floating-point flags before converting them to Governor's quota.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.requests_per_second.is_finite()
            || !(0.0..=1_000_000_000.0).contains(&self.requests_per_second)
        {
            return Err("rate-limit RPS must be greater than zero and at most one billion");
        }
        if !self.burst.is_finite() || !(1.0..=(u32::MAX as f64)).contains(&self.burst) {
            return Err("rate-limit burst must be between one and 4294967295");
        }
        Ok(())
    }
}

/// shared, in-memory token-bucket limiter keyed by an opaque principal/ip string.
pub struct RateLimiter {
    config: RateLimitConfig,
    limiter: DefaultKeyedRateLimiter<String>,
    clock: DefaultClock,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        let quota = quota_for(config);
        Self {
            config,
            limiter: DefaultKeyedRateLimiter::keyed(quota),
            clock: DefaultClock::default(),
        }
    }

    /// try to spend one token for `key`. returns `Ok(())` when allowed, or `Err(retry_after_secs)`
    /// with the wait before a token is available.
    fn check(&self, key: &str) -> Result<(), f64> {
        if needs_maintenance(self.limiter.len()) {
            // Governor's keyed store owns concurrent state and eviction. Pruning only entries that
            // are indistinguishable from a fresh bucket preserves enforcement while bounding idle
            // principal/IP churn without a home-grown bucket map.
            self.limiter.retain_recent();
        }
        let key = key.to_owned();
        self.limiter
            .check_key(&key)
            .map(|_| ())
            .map_err(|not_until| not_until.wait_time_from(self.clock.now()).as_secs_f64())
    }
}

/// Convert the historical floating-point flags into Governor's integral burst plus monotonic
/// refill interval. Startup validates these values before construction.
fn quota_for(config: RateLimitConfig) -> Quota {
    config
        .validate()
        .expect("rate-limit configuration was validated at startup");
    let burst = config.burst.ceil() as u32;
    let period = Duration::from_secs_f64(1.0 / config.requests_per_second);
    Quota::with_period(period)
        .expect("positive Governor refill period")
        .allow_burst(NonZeroU32::new(burst).expect("positive Governor burst"))
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

/// enrollment redemption uses the same strict unauthenticated throttle but a separate bucket, so
/// a legitimate enrollment cannot be starved by login traffic from the same NAT address.
pub fn check_enrollment_attempt(ip: IpAddr) -> Result<(), f64> {
    login_throttle().check(&format!("enroll:{ip}"))
}

/// paths exempt from rate limiting so health/metrics scrapers are never throttled.
fn is_exempt(path: &str) -> bool {
    matches!(path, "/health" | "/ready" | "/metrics")
}

/// derive the rate-limit key: the authenticated principal when present, else the connection ip.
fn rate_limit_key(req: &Request<Body>) -> String {
    if let Some(context) = req.extensions().get::<AuthContext>()
        && let Some(id) = context.principal_id
    {
        return format!("principal:{id}");
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
#[path = "rate_limit_tests.rs"]
mod tests;
