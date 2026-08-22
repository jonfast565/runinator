//! durable replica-directive orchestration.

use chrono::{DateTime, Duration, Utc};
use runinator_broker_core::{Broker, BrokerError};
use runinator_comm::{
    ActionTarget, AgentCommand, AgentDirectiveKind, AgentDirectiveRecord, AgentDirectiveResult,
};
use runinator_models::errors::SendableError;
use runinator_store::roles::ReplicaStore;
use uuid::Uuid;

pub async fn enqueue_agent_directive<T: ReplicaStore>(
    db: &T,
    replica_id: Uuid,
    kind: AgentDirectiveKind,
    expires_at: DateTime<Utc>,
) -> Result<AgentDirectiveRecord, SendableError> {
    db.enqueue_agent_directive(replica_id, kind, expires_at)
        .await
}

pub async fn fetch_agent_directive<T: ReplicaStore>(
    db: &T,
    directive_id: Uuid,
) -> Result<Option<AgentDirectiveRecord>, SendableError> {
    db.fetch_agent_directive(directive_id).await
}

pub async fn list_agent_directives<T: ReplicaStore>(
    db: &T,
    replica_id: Uuid,
    limit: i64,
) -> Result<Vec<AgentDirectiveRecord>, SendableError> {
    db.list_agent_directives(replica_id, limit).await
}

pub async fn complete_agent_directive<T: ReplicaStore>(
    db: &T,
    result: AgentDirectiveResult,
) -> Result<Option<AgentDirectiveRecord>, SendableError> {
    db.complete_agent_directive(result).await
}

pub async fn publish_due_agent_directives<T: ReplicaStore>(
    db: &T,
    broker: &dyn Broker,
    runtime_id: &str,
    limit: i64,
) -> Result<(), SendableError> {
    let now = Utc::now();
    db.expire_agent_directives(now).await?;
    let rows = db
        .claim_due_agent_directives(
            runtime_id.to_string(),
            now,
            now - Duration::seconds(30),
            limit,
        )
        .await?;
    for row in rows {
        let command = AgentCommand {
            directive_id: row.directive_id,
            replica_id: row.replica_id,
            target: ActionTarget::Replica {
                replica_id: row.replica_id,
            },
            kind: row.kind,
            issued_at: row.issued_at,
            expires_at: row.expires_at,
        };
        match broker.publish_agent(command).await {
            Ok(()) | Err(BrokerError::Duplicate(_)) => {
                db.mark_agent_directive_published(row.directive_id).await?;
            }
            Err(err) => {
                tracing::warn!(directive_id = %row.directive_id, "failed to publish agent directive: {err}");
            }
        }
    }
    Ok(())
}
