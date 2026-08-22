//! The retry policy applied to a failed effect, between its terminal result and its settlement.
//!
//! A node's `@retry(...)` is frozen into its `WorkflowEffectRequest::Action`, so the policy travels
//! with the effect and an in-flight run keeps the policy it was compiled with. The decision is made
//! here rather than in `runinator-runtime` because a retry never reaches the VM: the continuation
//! stays parked on the same effect across attempts, and only the dispatch is re-armed.

use chrono::{DateTime, Duration, Utc};
use runinator_models::workflow_vm::{WorkflowEffect, WorkflowEffectRequest, WorkflowEffectStatus};
use runinator_models::workflows::{WorkflowRetry, WorkflowStatus};

/// When the next attempt of `effect` may run, or `None` when the policy is exhausted, the terminal
/// is not retryable, or the effect carries no retry policy at all.
pub fn next_attempt_at(
    effect: &WorkflowEffect,
    status: WorkflowEffectStatus,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let retry = retry_policy(&effect.request)?;
    if !retry.retry_on.retryable(retryable_status(status)?) {
        return None;
    }
    // `attempt` is zero-based, so attempt 0 of `max_attempts: 3` has two retries left.
    if i64::from(effect.attempt) + 1 >= retry.max_attempts {
        return None;
    }
    Some(now + Duration::seconds(backoff_seconds(retry, effect.attempt)))
}

/// Only an action carries a retry policy; a timer or an external interaction has nothing to re-run.
fn retry_policy(request: &WorkflowEffectRequest) -> Option<&WorkflowRetry> {
    match request {
        WorkflowEffectRequest::Action { retry, .. } => Some(retry),
        _ => None,
    }
}

/// Map an effect terminal onto the node-run status the authored `retry_on` class is written against.
/// A rejection is an external decision and a cancel is an operator's, so neither is ever re-run.
fn retryable_status(status: WorkflowEffectStatus) -> Option<WorkflowStatus> {
    match status {
        WorkflowEffectStatus::Failed => Some(WorkflowStatus::Failed),
        WorkflowEffectStatus::TimedOut => Some(WorkflowStatus::TimedOut),
        _ => None,
    }
}

/// Exponential backoff from `backoff_base_seconds`, doubling per attempt and capped at
/// `backoff_max_seconds`; with `jitter` the delay is spread over `[delay / 2, delay]`.
fn backoff_seconds(retry: &WorkflowRetry, attempt: u32) -> i64 {
    let base = retry.backoff_base_seconds.max(0);
    let cap = retry.backoff_max_seconds.max(0);
    // saturating: a large `max_attempts` must not overflow the shift into a negative delay.
    let delay = base
        .checked_mul(1i64.checked_shl(attempt.min(62)).unwrap_or(i64::MAX))
        .unwrap_or(i64::MAX)
        .min(cap);
    if !retry.jitter || delay <= 1 {
        return delay;
    }
    let half = delay / 2;
    half + pseudo_random_below(delay - half)
}

/// A jitter source with no dependency of its own: the low bits of the current nanosecond, which is
/// all "spread a retry storm" needs.
fn pseudo_random_below(bound: i64) -> i64 {
    if bound <= 0 {
        return 0;
    }
    let nanos = i64::from(Utc::now().timestamp_subsec_nanos());
    nanos % bound
}

#[cfg(test)]
#[path = "effect_retry_tests.rs"]
mod tests;
