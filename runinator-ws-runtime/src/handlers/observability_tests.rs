//! runtime diagnostics ingress identity tests.

use super::*;
use chrono::Utc;
use runinator_models::diagnostics::RuntimeLogRecord;

#[test]
fn authenticated_system_identity_replaces_spoofed_source_fields() {
    let principal = uuid::Uuid::now_v7();
    let mut record = RuntimeLogRecord {
        event_id: uuid::Uuid::now_v7(),
        occurred_at: Utc::now(),
        source: "engine".into(),
        runtime_id: Some("someone-else".into()),
        replica_id: None,
        level: "info".into(),
        target: "test".into(),
        message: "hello".into(),
        dropped_before: 0,
        workflow_run_id: None,
        effect_id: None,
        trace_id: None,
    };

    bind_runtime_log_identity(&mut record, Some(SystemRole::Worker), Some(principal));

    assert_eq!(record.source, "worker");
    assert_eq!(
        record.runtime_id.as_deref(),
        Some(principal.to_string().as_str())
    );
}
