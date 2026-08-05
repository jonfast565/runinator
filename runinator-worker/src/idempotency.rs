//! reservation of an action node's declared idempotency key, taken before the provider is invoked.
//!
//! what this buys, precisely: once an execution completes under a key, any later delivery carrying
//! that key replays the recorded result instead of re-invoking the provider. because the result is
//! recorded *before* the status publish, a publish/flush failure that nacks the delivery no longer
//! re-runs the action's side effects — the redelivery replays instead (appendix A.7). a second
//! claimant on the same key is excluded while the first is still working.
//!
//! what it does not buy: the platform cannot know whether a side effect landed when a worker died
//! mid-invocation. that window is why the resolved key is also handed to the provider, so providers
//! with native idempotency (stripe-style request keys) can dedupe on it themselves (appendix A.1).

use runinator_api::{AsyncApiClient, ServiceLocator};
use runinator_comm::ActionCommand;
use runinator_models::orchestration::{IdempotencyClaim, IdempotentActionResult};
use runinator_models::value::Value;
use tracing::{info, warn};

/// what the worker should do with a delivery after consulting the key it declared.
pub enum IdempotencyGate {
    /// execute normally. carries the key to record against, when the action declared one.
    Execute { key: Option<String> },
    /// an execution already completed under this key; settle the node with `result` and do not
    /// invoke the provider.
    Replay { result: IdempotentActionResult },
    /// another node run holds a live reservation, so this delivery is a concurrent duplicate.
    Duplicate,
}

/// reserve `command`'s idempotency key, if it declares one. a transport failure fails open to
/// executing: a web-service blip must not wedge action execution, and the executor lease already
/// bounds concurrent duplicates.
pub async fn open_gate<L: ServiceLocator>(
    api_client: &AsyncApiClient<L>,
    command: &ActionCommand,
) -> IdempotencyGate {
    let Some(key) = command.idempotency_key.clone() else {
        return IdempotencyGate::Execute { key: None };
    };
    let claim = api_client
        .claim_idempotency_key(
            &key,
            command.workflow_node_run_id,
            command.action.timeout_seconds,
        )
        .await;
    match claim {
        Ok(IdempotencyClaim::Acquired) => IdempotencyGate::Execute {
            key: Some(key.clone()),
        },
        Ok(IdempotencyClaim::Completed { result }) => {
            match result.decode::<IdempotentActionResult>() {
                Ok(result) => {
                    info!(
                        node_run_id = %command.workflow_node_run_id,
                        idempotency_key = %key,
                        "replaying recorded result for an already-completed idempotency key"
                    );
                    IdempotencyGate::Replay { result }
                }
                // a completed row we cannot read is worse than useless: replaying nothing would
                // settle the node on a fabricated outcome, so execute and let the provider dedupe.
                Err(err) => {
                    warn!(
                        node_run_id = %command.workflow_node_run_id,
                        idempotency_key = %key,
                        "completed idempotency record is unreadable, executing instead: {}",
                        err
                    );
                    IdempotencyGate::Execute { key: Some(key) }
                }
            }
        }
        Ok(IdempotencyClaim::Held { owner_node_run_id }) => {
            info!(
                node_run_id = %command.workflow_node_run_id,
                owner_node_run_id = %owner_node_run_id,
                idempotency_key = %key,
                "skipping duplicate delivery: idempotency key held by another node run"
            );
            IdempotencyGate::Duplicate
        }
        Err(err) => {
            warn!(
                node_run_id = %command.workflow_node_run_id,
                idempotency_key = %key,
                "failed to claim idempotency key, executing without the reservation: {}",
                err
            );
            IdempotencyGate::Execute { key: Some(key) }
        }
    }
}

/// record a successful execution against the reserved key so a redelivery replays it. best-effort:
/// a failure here costs the replay guarantee for this key, not the run.
pub async fn record_success<L: ServiceLocator>(
    api_client: &AsyncApiClient<L>,
    key: &str,
    node_run_id: uuid::Uuid,
    result: &IdempotentActionResult,
) {
    let payload = match Value::encode(result) {
        Ok(payload) => payload,
        Err(err) => {
            warn!(node_run_id = %node_run_id, "failed to encode idempotency result: {}", err);
            return;
        }
    };
    if let Err(err) = api_client
        .complete_idempotency_key(key, node_run_id, payload)
        .await
    {
        warn!(
            node_run_id = %node_run_id,
            idempotency_key = %key,
            "failed to record idempotency result; a redelivery will re-execute: {}",
            err
        );
    }
}

/// free the reservation after a non-success outcome. a failed attempt must leave the key claimable,
/// or the node's own `.retry()` policy — and every later run — would be blocked behind a reservation
/// that no longer describes anything, until it aged out.
pub async fn release<L: ServiceLocator>(
    api_client: &AsyncApiClient<L>,
    key: &str,
    node_run_id: uuid::Uuid,
) {
    if let Err(err) = api_client.release_idempotency_key(key, node_run_id).await {
        warn!(
            node_run_id = %node_run_id,
            idempotency_key = %key,
            "failed to release idempotency reservation; it will age out instead: {}",
            err
        );
    }
}
