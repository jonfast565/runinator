#[allow(unused_imports)]
use super::*;

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
    pub fn validate(&self) -> Result<(), SendableError> {
        self.quota().map(|_| ())
    }

    pub(super) fn quota(&self) -> Result<Quota, SendableError> {
        if !self.requests_per_second.is_finite()
            || self.requests_per_second <= 0.0
            || self.requests_per_second > 1_000_000_000.0
        {
            return Err(
                errors::RATE_LIMIT_RPS.error("must be greater than zero and at most one billion")
            );
        }
        if !self.burst.is_finite() || !(1.0..=(u32::MAX as f64)).contains(&self.burst) {
            return Err(errors::RATE_LIMIT_BURST.error("must be between one and 4294967295"));
        }
        let burst = self.burst.ceil() as u32;
        let period = Duration::try_from_secs_f64(1.0 / self.requests_per_second)
            .map_err(|error| errors::RATE_LIMIT_QUOTA.error(error))?;
        // governor represents both the interval and burst tolerance in u64 nanoseconds.
        if period.as_nanos() * u128::from(burst) > u128::from(u64::MAX) {
            return Err(
                errors::RATE_LIMIT_QUOTA.error("burst refill duration exceeds the supported range")
            );
        }
        Ok(Quota::with_period(period)
            .ok_or_else(|| errors::RATE_LIMIT_QUOTA.error("must be at least one nanosecond"))?
            .allow_burst(NonZeroU32::new(burst).ok_or_else(|| errors::RATE_LIMIT_BURST.bare())?))
    }
}
