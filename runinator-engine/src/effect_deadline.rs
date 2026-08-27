//! The engine-side deadline backstop for a dispatched provider action.
//!
//! A worker enforces an action's `timeout_seconds` in its own process, which is exactly the process
//! that can vanish: a worker that dies mid-action, or an effect no worker's labels ever match,
//! leaves the effect non-terminal and its continuation parked with nothing left to settle it. This
//! arms a timer wake beside every dispatched action so the deadline is owned by something that
//! outlives the executor.
//!
//! It is a backstop, not the primary path. The wake is armed a grace period past the worker's own
//! deadline so a live worker always reports first, and losing the race is free: settling an effect
//! that already reached a terminal status (or moved to a later attempt) is rejected by the store
//! inside its own transaction, so a late deadline is an exact no-op.

use chrono::{DateTime, Utc};
use runinator_broker_core::{Broker, WakeMessage};
use runinator_comm::{EffectCommand, EffectResult, WakeCommand};
use runinator_models::workflow_vm::{
    DEFAULT_ACTION_TIMEOUT_SECONDS, WorkflowEffectRequest, WorkflowEffectStatus,
};
use tracing::{info, warn};

/// How far past the worker's own deadline the backstop fires.
///
/// The worker's clock starts when it receives the effect, so it always runs later than the
/// engine's; this margin covers publication, queueing, and the worker's idempotency claim, and
/// keeps the worker's more precise report the one that normally lands.
#[cfg(test)]
const DEADLINE_GRACE_SECONDS: i64 = 30;

/// The deadline wake for one dispatched effect, or `None` when the effect owns no action deadline.
///
/// Only provider actions are covered here. Infrastructure effects that complete at a known instant
/// (timers, an approval expiry, a gate deadline) are armed by the infrastructure effect host from
/// their own request, and the ones that park indefinitely by design (a signal, an input) have no
/// deadline to enforce.
#[cfg(test)]
pub(crate) fn deadline_wake(
    command: &EffectCommand,
    dispatched_at: DateTime<Utc>,
) -> Option<WakeCommand> {
    deadline_wake_with_grace(command, dispatched_at, DEADLINE_GRACE_SECONDS)
}

pub(crate) fn deadline_wake_with_grace(
    command: &EffectCommand,
    dispatched_at: DateTime<Utc>,
    grace_seconds: i64,
) -> Option<WakeCommand> {
    let WorkflowEffectRequest::Action {
        timeout_seconds, ..
    } = &command.request
    else {
        return None;
    };
    let budget = timeout_seconds
        .unwrap_or(DEFAULT_ACTION_TIMEOUT_SECONDS)
        .max(1);
    let due_at = dispatched_at + chrono::Duration::seconds(budget + grace_seconds.max(1));
    let mut result = EffectResult::status(
        command,
        WorkflowEffectStatus::TimedOut,
        None,
        // distinguishable from the worker's own timeout report on purpose: this message means
        // nothing ever answered, which is a different operational problem from an action that ran
        // and overran.
        Some(format!(
            "no result within {budget}s; the executing worker never reported"
        )),
    );
    result.timestamp = due_at;
    Some(WakeCommand::new(due_at, result, command.trace_id))
}

/// Arm the deadline for a just-published effect, if it has one.
///
/// Failures are logged and swallowed: this runs *after* the effect is published precisely so a
/// wake-channel problem can never stop the work it protects. A lost arming degrades to the
/// behaviour that existed before the backstop — the worker's own timeout, and nothing if the worker
/// dies — rather than halting dispatch.
#[cfg(test)]
pub(crate) async fn arm(
    broker: &dyn Broker,
    command: &EffectCommand,
    dispatched_at: DateTime<Utc>,
) {
    arm_with_grace(broker, command, dispatched_at, DEADLINE_GRACE_SECONDS).await;
}

pub(crate) async fn arm_with_grace(
    broker: &dyn Broker,
    command: &EffectCommand,
    dispatched_at: DateTime<Utc>,
    grace_seconds: i64,
) {
    let Some(wake) = deadline_wake_with_grace(command, dispatched_at, grace_seconds) else {
        return;
    };
    let due_at = wake.due_at;
    match broker
        .publish_wake(WakeMessage {
            dedupe_key: Some(wake.dedupe_key()),
            command: wake,
            enqueued_at: Utc::now(),
        })
        .await
    {
        // a duplicate means this attempt's deadline is already armed, which is what the
        // `effect_id:attempt` dedupe key is for: a redelivered dispatch re-arms nothing, while a
        // retry publishes under a new attempt and gets its own deadline.
        Ok(()) | Err(runinator_broker_core::BrokerError::Duplicate(_)) => {
            info!(
                effect_id = %command.effect_id,
                attempt = command.attempt,
                due_at = %due_at,
                "armed the action deadline backstop",
            );
        }
        Err(err) => {
            warn!(
                error = %err,
                effect_id = %command.effect_id,
                "failed to arm the action deadline backstop; this attempt is protected only by the worker's own timeout",
            );
        }
    }
}

#[cfg(test)]
#[path = "effect_deadline_tests.rs"]
mod tests;
