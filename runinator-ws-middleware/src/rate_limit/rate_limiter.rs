#[allow(unused_imports)]
use super::*;

pub struct RateLimiter {
    pub(super) config: RateLimitConfig,
    pub(super) limiter: DefaultKeyedRateLimiter<String>,
    pub(super) clock: DefaultClock,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        let quota = config
            .quota()
            .expect("rate-limit configuration was validated at startup");
        Self {
            config,
            limiter: DefaultKeyedRateLimiter::keyed(quota),
            clock: DefaultClock::default(),
        }
    }

    /// try to spend one token for `key`. returns `Ok(())` when allowed, or `Err(retry_after_secs)`
    /// with the wait before a token is available.
    pub(super) fn check(&self, key: &str) -> Result<(), f64> {
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
