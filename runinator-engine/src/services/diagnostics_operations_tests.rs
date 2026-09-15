//! runtime diagnostics correlation validation.

use std::sync::Arc;

use chrono::Utc;
use runinator_database::sqlite::SqliteDb;
use runinator_models::diagnostics::RuntimeLogRecord;
use runinator_store::DatabaseImpl;
use uuid::Uuid;

use super::DiagnosticsOperations;

#[tokio::test]
async fn rejects_an_unverified_workflow_run_correlation() {
    let path = std::env::temp_dir().join(format!("diagnostics-service-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let operations = DiagnosticsOperations::new(Arc::new(db));
    let record = RuntimeLogRecord {
        event_id: Uuid::now_v7(),
        occurred_at: Utc::now(),
        source: "worker".into(),
        runtime_id: None,
        replica_id: None,
        level: "error".into(),
        target: "test".into(),
        message: "failure".into(),
        dropped_before: 0,
        workflow_run_id: Some(Uuid::now_v7()),
        effect_id: None,
        trace_id: None,
    };

    let error = operations.record(vec![record]).await.unwrap_err();
    assert!(error.to_string().contains("unknown workflow run"));
}
