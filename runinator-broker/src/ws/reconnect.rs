//! exponential backoff with jitter for the ws client's reconnect loop. deliberately minimal: no
//! external `rand` dependency (jitter is mixed from the current time, which is random enough for
//! "don't thundering-herd reconnect after a server restart" — no cryptographic property needed here).

use std::time::Duration;

const INITIAL_MS: u64 = 500;
const MAX_MS: u64 = 30_000;

pub(crate) struct Backoff {
    attempt: u32,
}

impl Backoff {
    pub(crate) fn new() -> Self {
        Self { attempt: 0 }
    }

    /// back to the initial delay; call after a connection is used successfully.
    pub(crate) fn reset(&mut self) {
        self.attempt = 0;
    }

    /// the delay to wait before the next reconnect attempt, advancing the sequence.
    pub(crate) fn next_delay(&mut self) -> Duration {
        let exponent = self.attempt.min(6); // 500ms * 2^6 = 32s, already past the cap
        self.attempt = self.attempt.saturating_add(1);
        let capped_ms = INITIAL_MS.saturating_mul(1u64 << exponent).min(MAX_MS);
        let half = capped_ms / 2;
        let jitter_range = (capped_ms - half).max(1);
        let jitter_ms = jitter_nanos() % jitter_range;
        Duration::from_millis(half + jitter_ms)
    }
}

fn jitter_nanos() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0)
}
