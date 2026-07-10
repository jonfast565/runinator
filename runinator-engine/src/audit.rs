//! durable observability helpers: dead-letter persistence and the authn/authz audit trail.
//!
//! both are best-effort sinks. a failure to persist a dead letter or an audit row is logged but
//! never propagated, so it cannot take down the consumer or fail the request it describes.

use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::error_code_or_unknown;
use runinator_models::json;
use tracing::error;
use uuid::Uuid;

/// persist a dead-lettered broker message so a failed delivery leaves a durable record.
pub async fn persist_dead_letter<T: DatabaseImpl>(
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
#[allow(clippy::too_many_arguments)]
pub async fn record_audit<T: DatabaseImpl>(
    db: &T,
    actor_id: Option<Uuid>,
    actor_kind: &str,
    action: &str,
    outcome: AuditOutcome,
    resource_type: Option<&str>,
    resource_id: Option<Uuid>,
    detail: Option<&str>,
) {
    let record = json!({
        "actor_id": actor_id.map(|id| id.to_string()),
        "actor_kind": actor_kind,
        "action": action,
        "outcome": outcome.as_str(),
        "resource_type": resource_type,
        "resource_id": resource_id.map(|id| id.to_string()),
        "detail": detail,
    });
    if let Err(err) = db.record_audit_log(record).await {
        error!(
            action,
            error_code = error_code_or_unknown(err.as_ref()),
            "failed to persist audit log: {err}"
        );
    }
}
