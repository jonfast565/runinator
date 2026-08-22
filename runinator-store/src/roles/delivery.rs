//! delivery hygiene: the dead-letter log and the idempotency-key ledger.
//!
//! one of the role traits `DatabaseImpl` composes. the action-dispatch outbox this file was named
//! for is gone — the vm's effect dispatches live in [`super::WorkflowVmStore`] — but a worker still
//! needs somewhere to record an undeliverable payload and to claim a key exactly once.

use std::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_models::value::Value;
use runinator_models::{errors::SendableError, orchestration::IdempotencyClaim};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::runtime_store::RuntimeStore;

/// Core persistence operations for Runinator.
/// At-least-once delivery plumbing: the action-dispatch outbox, idempotency claims, and dead letters.
pub trait DeliveryStore: Send + Sync + 'static {
    /// Persist a dead-lettered broker message for later inspection/replay.
    fn record_dead_letter(
        &self,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch dead-letter rows, newest first, with an optional channel filter.
    fn fetch_dead_letters(
        &self,
        channel: Option<String>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Store a result for an idempotency key.
    fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch the result for an idempotency key if it exists.
    fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Reserve an idempotency key for an action node before its provider is invoked. Decides in one
    /// statement, so of two concurrent claimants exactly one is `Acquired`. Returns `Completed` when
    /// an execution already finished under this key (the caller replays that result instead of
    /// executing), and `Held` when a *different* node run owns an unfinished reservation. A caller
    /// re-claiming its own unfinished reservation is `Acquired` again: a redelivery of the same node
    /// run may have crashed before doing any work, and refusing it would strand the node. A
    /// reservation older than `stale_before` is likewise takeable, so a worker that died holding one
    /// does not block the key forever.
    fn claim_idempotency_key(
        &self,
        scope: String,
        key: String,
        owner_node_run_id: Uuid,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> impl Future<Output = Result<IdempotencyClaim, SendableError>> + Send;

    /// Record a completed execution against a key the caller reserved, making it replayable. A no-op
    /// unless `owner_node_run_id` still owns the reservation, so a late write from a superseded
    /// claimant cannot overwrite the winner's result.
    fn complete_idempotency_key(
        &self,
        scope: String,
        key: String,
        owner_node_run_id: Uuid,
        result: Value,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Free the caller's own unfinished reservation, so a non-success outcome does not hold the key
    /// for the whole staleness window. A no-op unless the caller still owns an uncompleted row, so it
    /// can never clear a recorded result or another claimant's live reservation.
    fn release_idempotency_key(
        &self,
        scope: String,
        key: String,
        owner_node_run_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
