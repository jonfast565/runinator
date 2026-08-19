//! the durable action-dispatch outbox: dedupe on enqueue, publish-state tracking, malformed command
//! rejection, and per-publisher claim leases.

use super::*;

#[tokio::test]
async fn action_dispatch_outbox_is_idempotent_and_tracks_publish_state() {
    let path = std::env::temp_dir().join(format!(
        "runinator-action-dispatches-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let command = action_command(Uuid::new_v4(), Uuid::new_v4(), "node-a");

    let first = db
        .enqueue_action_dispatch("dispatch-key".into(), command.clone())
        .await
        .unwrap();
    let second = db
        .enqueue_action_dispatch("dispatch-key".into(), command.clone())
        .await
        .unwrap();

    assert_eq!(first.id, second.id);
    let pending = db.fetch_pending_action_dispatches(10).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].command.command_id, command.command_id);
    let snapshot = db.action_dispatch_queue_snapshot(Utc::now()).await.unwrap();
    assert_eq!(snapshot.depth, 1);
    assert_eq!(snapshot.claimed, 0);
    assert!(snapshot.oldest_enqueued_at.is_some());

    db.mark_action_dispatch_failed(first.id, "broker unavailable".into())
        .await
        .unwrap();
    let pending = db.fetch_pending_action_dispatches(10).await.unwrap();
    assert_eq!(pending[0].attempts, 1);
    assert_eq!(pending[0].last_error.as_deref(), Some("broker unavailable"));

    db.mark_action_dispatch_published(first.id).await.unwrap();
    assert!(
        db.fetch_pending_action_dispatches(10)
            .await
            .unwrap()
            .is_empty()
    );
    let snapshot = db.action_dispatch_queue_snapshot(Utc::now()).await.unwrap();
    assert_eq!(snapshot.depth, 0);
    assert_eq!(snapshot.claimed, 0);
    assert!(snapshot.oldest_enqueued_at.is_none());

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn malformed_action_dispatch_command_returns_error() {
    let path = std::env::temp_dir().join(format!(
        "runinator-action-dispatches-malformed-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let command = action_command(Uuid::new_v4(), Uuid::new_v4(), "node-a");
    let dispatch = db
        .enqueue_action_dispatch("dispatch-key".into(), command)
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_action_dispatches SET command_json = ? WHERE id = ?")
        .bind("{")
        .bind(dispatch.id)
        .execute(db.pool())
        .await
        .unwrap();

    let err = db
        .fetch_pending_action_dispatches(10)
        .await
        .expect_err("malformed action dispatch command should return an error");
    assert!(
        err.to_string()
            .contains("database.action_dispatch.invalid_command_json")
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn action_dispatch_claims_respect_publisher_leases() {
    let path = std::env::temp_dir().join(format!(
        "runinator-action-dispatch-claim-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let node_run = create_node_run(&db).await;
    let command = action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    let dispatch = db
        .enqueue_action_dispatch("dispatch-key".into(), command)
        .await
        .unwrap();

    let first = db
        .claim_pending_action_dispatches(
            "scheduler-a".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, dispatch.id);

    let second = db
        .claim_pending_action_dispatches(
            "scheduler-b".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert!(second.is_empty());

    db.mark_action_dispatch_failed(dispatch.id, "publish failed".into())
        .await
        .unwrap();
    let retry = db
        .claim_pending_action_dispatches(
            "scheduler-b".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].claimed_by.as_deref(), Some("scheduler-b"));

    let _ = fs::remove_file(path);
}
