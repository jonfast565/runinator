//! durable observability helpers: dead-letter persistence and the authn/authz audit trail.
//!
//! both are best-effort sinks. a failure to persist a dead letter or an audit row is logged but
//! never propagated, so it cannot take down the consumer or fail the request it describes.

use runinator_models::errors::error_code_or_unknown;
use runinator_models::json;
use runinator_store::{RuntimeStore, roles::DeliveryStore};
use tracing::error;
use uuid::Uuid;

/// persist a dead-lettered broker message so a failed delivery leaves a durable record.
pub async fn persist_dead_letter<T: DeliveryStore>(
    db: &T,
    channel: &str,
    event_id: Option<Uuid>,
    dedupe_key: Option<String>,
    attempts: u32,
    error: &str,
    payload: serde_json::Value,
) {
    let record = json!({
        "channel": channel,
        "event_id": event_id.map(|id| id.to_string()),
        "dedupe_key": dedupe_key,
        "attempts": attempts as i64,
        "error": error,
        "payload": payload,
    });
    if let Err(err) = db.record_dead_letter(record).await {
        error!(
            channel,
            error_code = error_code_or_unknown(err.as_ref()),
            "failed to persist dead letter: {err}"
        );
    }
}

/// outcome of an audited action, used for the `outcome` column.
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

/// The actor, target, and outcome of one audit-log entry.
pub struct AuditEntry<'a> {
    pub actor_id: Option<Uuid>,
    pub actor_kind: &'a str,
    pub action: &'a str,
    pub outcome: AuditOutcome,
    pub resource_type: Option<&'a str>,
    pub resource_id: Option<Uuid>,
    pub detail: Option<&'a str>,
}

impl<'a> AuditEntry<'a> {
    pub fn new(
        actor_id: Option<Uuid>,
        actor_kind: &'a str,
        action: &'a str,
        outcome: AuditOutcome,
        resource_type: Option<&'a str>,
        resource_id: Option<Uuid>,
        detail: Option<&'a str>,
    ) -> Self {
        Self {
            actor_id,
            actor_kind,
            action,
            outcome,
            resource_type,
            resource_id,
            detail,
        }
    }
}

impl AuditOutcome {
    fn as_str(&self) -> &'static str {
        match self {
            AuditOutcome::Success => "success",
            AuditOutcome::Failure => "failure",
            AuditOutcome::Denied => "denied",
        }
    }
}

/// append an audit-log entry. `actor_id`/`actor_kind` describe the principal; `resource_*` are
/// optional and name the affected resource for authz decisions.
pub async fn record_audit<T: RuntimeStore>(db: &T, entry: AuditEntry<'_>) {
    let record = json!({
        "actor_id": entry.actor_id.map(|id| id.to_string()),
        "actor_kind": entry.actor_kind,
        "action": entry.action,
        "outcome": entry.outcome.as_str(),
        "resource_type": entry.resource_type,
        "resource_id": entry.resource_id.map(|id| id.to_string()),
        "detail": entry.detail,
    });
    if let Err(err) = db.record_audit_log(record).await {
        error!(
            action = entry.action,
            error_code = error_code_or_unknown(err.as_ref()),
            "failed to persist audit log: {err}"
        );
    }
}
