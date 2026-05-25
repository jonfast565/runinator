use std::{collections::HashMap, sync::Arc, time::Duration};

use log::{error, info, warn};
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
#[cfg(test)]
use runinator_models::errors::{RuntimeError, SendableError};
use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    events::{EventSender, emit_workflow_node_run},
    repository, stability,
};

const RESULT_CONSUMER_ID: &str = "runinator-ws-results";
const DEFAULT_MAX_RESULT_ATTEMPTS: u32 = 3;
const DEFAULT_RESULT_RETRY_BACKOFF: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResultConsumerPolicy {
    max_attempts: u32,
    retry_backoff: Duration,
}

impl Default for ResultConsumerPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_RESULT_ATTEMPTS,
            retry_backoff: DEFAULT_RESULT_RETRY_BACKOFF,
        }
    }
}

impl ResultConsumerPolicy {
    #[cfg(test)]
    pub(crate) fn new(max_attempts: u32, retry_backoff: Duration) -> Self {
        Self {
            max_attempts,
            retry_backoff,
        }
    }
}

pub(crate) async fn run_result_consumer<T: DatabaseImpl>(
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

pub(crate) async fn run_result_consumer_with_policy<T: DatabaseImpl>(
    db: Arc<T>,
    broker: Arc<dyn Broker>,
    events: EventSender,
    shutdown: Arc<Notify>,
    policy: ResultConsumerPolicy,
) {
    info!("Workflow result consumer started");
    let mut attempts = HashMap::<Uuid, u32>::new();
    loop {
        let delivery = tokio::select! {
            _ = shutdown.notified() => {
                info!("Workflow result consumer shutting down");
                return;
            }
            result = broker.receive_result(RESULT_CONSUMER_ID) => {
                match result {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        stability::result_receive_error();
                        error!("Failed to receive workflow result event: {}", err);
                        continue;
                    }
                }
            }
        };

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
                        "Failed to ack workflow result event {}: {}",
                        delivery.event.event_id, err
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
                    "Failed to persist workflow result event {} on attempt {}: {}",
                    delivery.event.event_id, attempt_count, err
                );
                if attempt_count >= policy.max_attempts.max(1) {
                    stability::result_event_dead_lettered();
                    attempts.remove(&delivery.event.event_id);
                    warn!(
                        "Dead-lettering workflow result event {} after {} attempt(s)",
                        delivery.event.event_id, attempt_count
                    );
                    if let Err(err) = broker
                        .ack_result(RESULT_CONSUMER_ID, delivery.delivery_id)
                        .await
                    {
                        error!(
                            "Failed to ack dead-lettered workflow result event {}: {}",
                            delivery.event.event_id, err
                        );
                    }
                    continue;
                }

                stability::result_event_retried();
                tokio::time::sleep(policy.retry_backoff).await;
                if let Err(err) = broker
                    .nack_result(RESULT_CONSUMER_ID, delivery.delivery_id)
                    .await
                {
                    error!(
                        "Failed to nack workflow result event {}: {}",
                        delivery.event.event_id, err
                    );
                }
            }
        }
    }
}

#[cfg(not(test))]
async fn apply_result_event<T: DatabaseImpl>(
    db: &T,
    event: &runinator_comm::WorkflowResultEvent,
) -> Result<bool, runinator_models::errors::SendableError> {
    repository::apply_workflow_result_event(db, event).await
}

#[cfg(test)]
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
