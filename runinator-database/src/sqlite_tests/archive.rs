//! archive marks and the sweep that deletes the source rows, including the unread-notification rows
//! it must leave alone.

use super::*;

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
async fn archive_marking_skips_unread_notifications() {
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
    db.mark_notification_read(read.id).await.unwrap();
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
        1
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
    assert_eq!(marks[0].primary_key, read.id);

    let _ = std::fs::remove_file(path);
}
