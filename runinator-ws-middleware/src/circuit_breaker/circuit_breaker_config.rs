#[allow(unused_imports)]
use super::*;

/// Runtime policy for all inbound circuit families on one API replica.
#[derive(Debug, Clone, Copy)]
pub struct CircuitBreakerConfig {
    pub enabled: bool,
    /// Failure fraction that opens a closed circuit once its sample is eligible.
    pub failure_rate_threshold: f64,
    /// Minimum number of handler calls sampled before evaluating the failure rate.
    pub minimum_number_of_calls: usize,
    /// Number of calls retained by the count-based sliding window.
    pub sliding_window_size: usize,
    /// How long an open circuit rejects requests before a half-open probe is admitted.
    pub cooldown: Duration,
    /// Number of simultaneous recovery probes admitted while half-open.
    pub permitted_calls_in_half_open: usize,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            failure_rate_threshold: 0.5,
            minimum_number_of_calls: 20,
            sliding_window_size: 100,
            cooldown: Duration::from_secs(30),
            permitted_calls_in_half_open: 1,
        }
    }
}

impl CircuitBreakerConfig {
    /// Check CLI/environment-derived values before the server begins accepting traffic.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.failure_rate_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.failure_rate_threshold)
        {
            return Err("circuit breaker failure-rate threshold must be between 0 and 1");
        }
        if self.minimum_number_of_calls == 0 {
            return Err("circuit breaker minimum calls must be greater than zero");
        }
        if self.sliding_window_size == 0 {
            return Err("circuit breaker window size must be greater than zero");
        }
        if self.minimum_number_of_calls > self.sliding_window_size {
            return Err("circuit breaker minimum calls cannot exceed the window size");
        }
        if self.cooldown.is_zero() {
            return Err("circuit breaker cooldown must be greater than zero");
        }
        if self.permitted_calls_in_half_open == 0 {
            return Err("circuit breaker half-open probe count must be greater than zero");
        }
        Ok(())
    }
}
