use super::*;
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use uuid::Uuid;

/// record a durable intent to publish one action command. the dedupe key is what keeps a retried
/// caller from enqueuing the same action twice.
pub async fn enqueue_action_dispatch<T: DatabaseImpl>(
    db: &T,
    dedupe_key: String,
    command: ActionCommand,
) -> Result<ActionDispatchRecord, SendableError> {
    db.enqueue_action_dispatch(dedupe_key, command).await
}

pub async fn fetch_pending_action_dispatches<T: DatabaseImpl>(
    db: &T,
    limit: i64,
) -> Result<Vec<ActionDispatchRecord>, SendableError> {
    db.fetch_pending_action_dispatches(limit).await
}

pub async fn claim_pending_action_dispatches<T: DatabaseImpl>(
    db: &T,
    publisher_id: String,
    lease_until: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<ActionDispatchRecord>, SendableError> {
    db.claim_pending_action_dispatches(publisher_id, Utc::now(), lease_until, limit)
        .await
}

pub async fn mark_action_dispatch_published<T: DatabaseImpl>(
    db: &T,
    dispatch_id: Uuid,
) -> Result<(), SendableError> {
    db.mark_action_dispatch_published(dispatch_id).await
}

pub async fn mark_action_dispatch_failed<T: DatabaseImpl>(
    db: &T,
    dispatch_id: Uuid,
    error: String,
) -> Result<(), SendableError> {
    db.mark_action_dispatch_failed(dispatch_id, error).await
}

/// drain durable action-dispatch intents and publish them to the broker action channel. moved into
/// the engine (which owns the database and the reducer) so the waker no longer relays them.
pub async fn publish_pending_action_dispatches<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    publisher_id: &str,
    lease_seconds: i64,
    limit: i64,
) -> Result<(), SendableError> {
    let lease_until = Utc::now() + Duration::seconds(lease_seconds);
    let dispatches =
        claim_pending_action_dispatches(db, publisher_id.to_string(), lease_until, limit).await?;
    for dispatch in dispatches {
        let dispatch_id = dispatch.id;
        let message = BrokerMessage {
            command: dispatch.command,
            dedupe_key: Some(dispatch.dedupe_key),
            enqueued_at: Utc::now(),
        };
        match broker.publish(message).await {
            Ok(()) | Err(BrokerError::Duplicate(_)) => {
                mark_action_dispatch_published(db, dispatch_id).await?;
            }
            Err(err) => {
                mark_action_dispatch_failed(db, dispatch_id, err.to_string()).await?;
            }
        }
    }
    Ok(())
}
