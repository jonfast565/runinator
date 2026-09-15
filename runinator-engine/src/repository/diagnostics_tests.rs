//! durable runtime diagnostics filtering and pagination.

use super::*;
use chrono::Utc;
use runinator_database::sqlite::SqliteDb;
use runinator_store::DatabaseImpl;

async fn diagnostics_db() -> SqliteDb {
    let path = std::env::temp_dir().join(format!("runtime-diagnostics-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    db
}

fn record(source: &str, level: &str, message: &str) -> RuntimeLogRecord {
    RuntimeLogRecord {
        event_id: Uuid::now_v7(),
        occurred_at: Utc::now(),
        source: source.into(),
        runtime_id: None,
        replica_id: None,
        level: level.into(),
        target: "diagnostics-test".into(),
        message: message.into(),
        dropped_before: 0,
        workflow_run_id: None,
        effect_id: None,
        trace_id: None,
    }
}

#[tokio::test]
async fn filters_and_pages_retained_runtime_logs() {
    let db = diagnostics_db().await;
    record_runtime_logs(
        &db,
        vec![
            record("worker-a", "info", "started"),
            record("worker-b", "error", "pack import failed"),
            record("worker-b", "warn", "retry disabled"),
        ],
    )
    .await
    .unwrap();

    let query = RuntimeLogQuery {
        source: Some("worker-b".into()),
        text: Some("i".into()),
        limit: Some(1),
        ..Default::default()
    };
    let first = fetch_runtime_logs(&db, &query, None, 1, 86_400)
        .await
        .unwrap();
    assert_eq!(first.records.len(), 1);
    assert!(first.next_cursor.is_some());

    let second = fetch_runtime_logs(
        &db,
        &RuntimeLogQuery {
            cursor: first.next_cursor,
            ..query
        },
        None,
        1,
        86_400,
    )
    .await
    .unwrap();
    assert_eq!(second.records.len(), 1);
    assert_ne!(first.records[0].event_id, second.records[0].event_id);
}
