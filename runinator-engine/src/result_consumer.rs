use std::{collections::HashMap, sync::Arc, time::Duration};

use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::error_code_or_unknown;
#[cfg(any(test, feature = "test-support"))]
use runinator_models::errors::{RuntimeError, SendableError};
use tokio::sync::Notify;
use tracing::{Instrument, error, info, warn};
use uuid::Uuid;

use crate::{
    audit::persist_dead_letter,
    events::{EventSender, emit_workflow_node_run},
    repository, stability,
};

const RESULT_CONSUMER_ID: &str = "runinator-ws-results";
const DEFAULT_MAX_RESULT_ATTEMPTS: u32 = 3;
const DEFAULT_RESULT_RETRY_BACKOFF: Duration = Duration::from_millis(250);
const DEFAULT_RESULT_MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub struct ResultConsumerPolicy {
    max_attempts: u32,
    retry_backoff: Duration,
    max_backoff: Duration,
}

impl Default for ResultConsumerPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_RESULT_ATTEMPTS,
            retry_backoff: DEFAULT_RESULT_RETRY_BACKOFF,
            max_backoff: DEFAULT_RESULT_MAX_BACKOFF,
        }
    }
}

impl ResultConsumerPolicy {
    /// build a policy with an explicit attempt cap and base retry backoff; used by callers that
    /// tune the consumer (and by cross-crate tests) rather than taking the environment default.
    pub fn new(max_attempts: u32, retry_backoff: Duration) -> Self {
        Self {
            max_attempts,
            retry_backoff,
            max_backoff: DEFAULT_RESULT_MAX_BACKOFF,
        }
    }

    /// compute the delay before the next retry with exponential backoff and full jitter.
    ///
    /// the base doubles per attempt (`base * 2^(attempt-1)`), is capped at `max_backoff`, then a
    /// uniformly random fraction in `[0, capped]` is taken (full jitter) to avoid thundering-herd
    /// retries when many events fail at once. `attempt` is 1-based (the first retry).
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        let base = self.retry_backoff.as_millis().max(1) as u64;
        let max = self.max_backoff.as_millis().max(1) as u64;
        // shift by attempt-1, saturating so a large attempt count cannot overflow.
        let shift = attempt.saturating_sub(1).min(32);
        let exp = base.saturating_mul(1u64 << shift).min(max);
        let jitter = jitter_millis(exp);
        Duration::from_millis(jitter)
    }
}

/// return a pseudo-random value in `[0, ceiling]` using process time as the entropy source.
///
/// retry jitter does not need a cryptographic rng, so we avoid a new dependency and derive the
/// value from the high-resolution clock.
fn jitter_millis(ceiling: u64) -> u64 {
    if ceiling == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos % (ceiling + 1)
}

pub async fn run_result_consumer<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    shutdown: Arc<Notify>,
) {
    run_result_consumer_with_policy(
        db,
        broker,
        events,
        shutdown,
        ResultConsumerPolicy::default(),
    )
    .await;
}

pub async fn run_result_consumer_with_policy<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    shutdown: Arc<Notify>,
    policy: ResultConsumerPolicy,
) {
    info!("workflow result consumer started");
    let mut attempts = HashMap::<Uuid, u32>::new();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                info!("workflow result consumer shutting down");
                return;
            }
            result = broker.receive_result(RESULT_CONSUMER_ID) => {
                match result {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        stability::result_receive_error();
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to receive workflow result event: {}", err
                        );
                        // back off so an unreachable broker does not spin this loop hot.
                        tokio::select! {
                            _ = shutdown.notified() => {
                                info!("workflow result consumer shutting down");
                                return;
                            }
                            _ = tokio::time::sleep(policy.retry_backoff) => {}
                        }
                        continue;
                    }
                }
            }
        };

        // re-parent this delivery's processing onto the trace the worker carried the result back on,
        // so a stuck/failed result apply is correlatable with the dispatch and execution that produced it.
        let span = tracing::info_span!(
            "result_event",
            trace_id = %delivery.event.trace_id,
            run_id = %delivery.event.workflow_run_id,
            node_run_id = %delivery.event.workflow_node_run_id,
            event_id = %delivery.event.event_id,
        );
        async {
            let node_run_id = delivery.event.workflow_node_run_id;
            match apply_result_event(db.as_ref(), &delivery.event).await {
                Ok(applied) => {
                    stability::result_event_applied(applied);
                    attempts.remove(&delivery.event.event_id);
                    emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
                    if let Err(err) = broker
                        .ack_result(RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to ack workflow result event: {}", err
                        );
                    }
                }
                Err(err) => {
                    let attempt_count = {
                        let attempt = attempts.entry(delivery.event.event_id).or_insert(0);
                        *attempt += 1;
                        *attempt
                    };
                    error!(
                        attempt = attempt_count,
                        error_code = error_code_or_unknown(err.as_ref()),
                        "failed to persist workflow result event: {}",
                        err
                    );
                    if attempt_count >= policy.max_attempts.max(1) {
                        stability::result_event_dead_lettered();
                        attempts.remove(&delivery.event.event_id);
                        warn!(
                            attempts = attempt_count,
                            "dead-lettering workflow result event"
                        );
                        persist_dead_letter(
                            db.as_ref(),
                            "result",
                            Some(delivery.event.event_id),
                            Some(delivery.dedupe_key.clone()),
                            attempt_count,
                            &err.to_string(),
                            serde_json::to_value(&delivery.event).unwrap_or_default(),
                        )
                        .await;
                        if let Err(err) = broker
                            .ack_result(RESULT_CONSUMER_ID, delivery.delivery_id)
                            .await
                        {
                            error!(
                                error_code = error_code_or_unknown(&err),
                                "failed to ack dead-lettered workflow result event: {}", err
                            );
                        }
                        return;
                    }

                    stability::result_event_retried();
                    tokio::time::sleep(policy.backoff_for(attempt_count)).await;
                    if let Err(err) = broker
                        .nack_result(RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to nack workflow result event: {}", err
                        );
                    }
                }
            }
        }
        .instrument(span)
        .await;
    }
}

#[cfg(not(any(test, feature = "test-support")))]
async fn apply_result_event<T: DatabaseImpl>(
    db: &T,
    event: &runinator_comm::WorkflowResultEvent,
) -> Result<bool, runinator_models::errors::SendableError> {
    repository::apply_workflow_result_event(db, event).await
}

#[cfg(any(test, feature = "test-support"))]
async fn apply_result_event<T: DatabaseImpl>(
    db: &T,
    event: &runinator_comm::WorkflowResultEvent,
) -> Result<bool, SendableError> {
    if event.node_id == "__force_result_persist_failure__" {
        return Err(Box::new(RuntimeError::new(
            "test.result_consumer.persist".into(),
            "forced result persistence failure".into(),
        )));
    }
    repository::apply_workflow_result_event(db, event).await
}
