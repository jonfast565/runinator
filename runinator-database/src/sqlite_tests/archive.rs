//! archive marks and the sweep that deletes source rows, including stale unread notifications.

use super::*;
use std::collections::BTreeSet;

use runinator_store::archive::{DATABASE_TABLE_POLICIES, TableDataPolicy};
use sqlx::Row;

#[tokio::test]
async fn archive_marks_are_idempotent_and_sweep_deletes_source_rows() {
    let path = std::env::temp_dir().join(format!(
        "runinator-archive-dlq-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let old = db
        .record_dead_letter(runinator_models::json!({
            "channel": "ingress",
            "attempts": 3,
            "error": "old",
            "payload": {"kind": "old"},
        }))
        .await
        .unwrap();
    let recent = db
        .record_dead_letter(runinator_models::json!({
            "channel": "ingress",
            "attempts": 1,
            "error": "recent",
            "payload": {"kind": "recent"},
        }))
        .await
        .unwrap();
    let old_id = Uuid::parse_str(old.get("id").and_then(Value::as_str).unwrap()).unwrap();
    let recent_id = Uuid::parse_str(recent.get("id").and_then(Value::as_str).unwrap()).unwrap();
    let old_timestamp = (Utc::now() - Duration::days(100)).timestamp();
    let recent_timestamp = Utc::now().timestamp();
    sqlx::query("UPDATE dead_letters SET created_at = ? WHERE id = ?")
        .bind(old_timestamp)
        .bind(old_id)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query("UPDATE dead_letters SET created_at = ? WHERE id = ?")
        .bind(recent_timestamp)
        .bind(recent_id)
        .execute(db.pool())
        .await
        .unwrap();

    let cutoff = Utc::now() - Duration::days(90);
    assert_eq!(
        db.mark_archive_candidates(ArchiveTable::DeadLetters, cutoff, 100)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.mark_archive_candidates(ArchiveTable::DeadLetters, cutoff, 100)
            .await
            .unwrap(),
        0
    );

    let marks = db
        .claim_archive_marks(
            "archiver-a".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(60),
            100,
        )
        .await
        .unwrap();
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].primary_key, old_id);

    let rows = db.fetch_archive_rows(marks).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].table, ArchiveTable::DeadLetters);
    assert!(
        rows[0]
            .row
            .get("payload")
            .and_then(Value::as_str)
            .unwrap()
            .contains("old")
    );
    let mark_ids = rows.iter().map(|row| row.mark_id).collect::<Vec<_>>();
    assert_eq!(db.delete_archive_rows(rows).await.unwrap(), 1);
    assert_eq!(db.complete_archive_marks(mark_ids).await.unwrap(), 1);

    let remaining = db.fetch_dead_letters(None, 100).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(
        remaining[0].get("error").and_then(Value::as_str),
        Some("recent")
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn archive_marking_includes_stale_unread_notifications() {
    let path = std::env::temp_dir().join(format!(
        "runinator-archive-notifications-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let unread = db
        .create_notification(&NewNotification {
            channel: "ui".into(),
            severity: "info".into(),
            title: "unread".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let read = db
        .create_notification(&NewNotification {
            channel: "ui".into(),
            severity: "info".into(),
            title: "read".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    db.mark_notification_read(None, read.id, Uuid::now_v7())
        .await
        .unwrap();
    let old_timestamp = (Utc::now() - Duration::days(40)).timestamp();
    for id in [unread.id, read.id] {
        sqlx::query("UPDATE notifications SET created_at = ? WHERE id = ?")
            .bind(old_timestamp)
            .bind(id)
            .execute(db.pool())
            .await
            .unwrap();
    }

    let cutoff = Utc::now() - Duration::days(30);
    assert_eq!(
        db.mark_archive_candidates(ArchiveTable::Notifications, cutoff, 100)
            .await
            .unwrap(),
        2
    );
    let marks = db
        .claim_archive_marks(
            "archiver-a".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(60),
            100,
        )
        .await
        .unwrap();
    assert_eq!(marks.len(), 2);
    assert_eq!(
        marks
            .iter()
            .map(|mark| mark.primary_key)
            .collect::<std::collections::HashSet<_>>(),
        [unread.id, read.id].into_iter().collect()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn every_runtime_growth_table_has_a_valid_archive_candidate_query() {
    let path = std::env::temp_dir().join(format!(
        "runinator-archive-coverage-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    for table in ArchiveTable::ALL {
        assert_eq!(
            db.mark_archive_candidates(table, Utc::now(), 10)
                .await
                .unwrap_or_else(|error| panic!("{table} has an invalid retention query: {error}")),
            0,
            "empty {table} should have no archive candidates"
        );
    }

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn every_database_table_has_one_lifecycle_policy() {
    let path = std::env::temp_dir().join(format!(
        "runinator-archive-policy-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let schema_tables = sqlx::query(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'
         ORDER BY name",
    )
    .fetch_all(db.pool())
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.get::<String, _>("name"))
    .collect::<BTreeSet<_>>();
    let policy_tables = DATABASE_TABLE_POLICIES
        .iter()
        .map(|entry| entry.table.to_string())
        .collect::<BTreeSet<_>>();

    assert_eq!(schema_tables, policy_tables);
    assert_eq!(policy_tables.len(), DATABASE_TABLE_POLICIES.len());

    let cold_tables = DATABASE_TABLE_POLICIES
        .iter()
        .filter(|entry| entry.policy == TableDataPolicy::ColdArchive)
        .map(|entry| entry.table)
        .collect::<BTreeSet<_>>();
    let implemented_tables = ArchiveTable::ALL
        .iter()
        .map(|table| table.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(cold_tables, implemented_tables);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn cold_archives_preserve_every_source_column() {
    let path = std::env::temp_dir().join(format!(
        "runinator-archive-columns-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    for table in ArchiveTable::ALL {
        let rows = sqlx::query(&format!("PRAGMA table_info({})", table.as_str()))
            .fetch_all(db.pool())
            .await
            .unwrap();
        let schema_columns = rows
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<BTreeSet<_>>();
        let archive_columns = crate::operations::archived_column_names(table)
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            schema_columns, archive_columns,
            "{table} archive mapping must preserve every column"
        );
    }

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn housekeeping_prunes_its_own_ledger_and_ephemeral_security_state() {
    let path = std::env::temp_dir().join(format!(
        "runinator-archive-housekeeping-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let old = (Utc::now() - Duration::days(40)).timestamp();
    let mark_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO archive_marks (id, table_name, primary_key, created_at, eligible_before, archive_day, status, attempts, marked_at, archived_at) VALUES (?, 'dead_letters', ?, ?, ?, '2026-01-01', 'archived', 1, ?, ?)",
    )
    .bind(mark_id)
    .bind(Uuid::now_v7().to_string())
    .bind(old)
    .bind(old)
    .bind(old)
    .bind(old)
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query("INSERT INTO workflow_cooldowns (name, last_run_at) VALUES ('old-key', ?)")
        .bind(old)
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO workflow_mutexes (name, holder_run_id, holder_continuation_id, acquired_at, hold_deadline, overdue_at, updated_at) VALUES ('old-mutex', NULL, NULL, NULL, NULL, NULL, ?)",
    )
    .bind(old)
    .execute(db.pool())
    .await
    .unwrap();

    assert_eq!(
        db.prune_completed_archive_marks(Utc::now() - Duration::days(30), 10)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.prune_workflow_cooldowns(Utc::now() - Duration::days(30), 10)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.prune_workflow_mutexes(Utc::now() - Duration::days(30), 10)
            .await
            .unwrap(),
        1
    );

    let _ = std::fs::remove_file(path);
}
