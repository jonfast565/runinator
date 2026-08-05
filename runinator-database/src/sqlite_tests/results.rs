//! applying worker result events: chunk idempotence, and the guards that stop a late or superseded
//! attempt from regressing a terminal status.

use super::*;

#[tokio::test]
async fn apply_workflow_result_event_is_idempotent_for_chunks() {
    let path = std::env::temp_dir().join(format!(
        "runinator-result-events-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let node_run = create_node_run(&db).await;
    let command = action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    let event = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );

    assert!(db.apply_workflow_result_event(&event).await.unwrap());
    assert!(!db.apply_workflow_result_event(&event).await.unwrap());

    let chunks = db
        .fetch_workflow_node_run_chunks(node_run.id, None, 100)
        .await
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content, "hello");

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn apply_workflow_result_event_does_not_regress_terminal_status() {
    let path = std::env::temp_dir().join(format!(
        "runinator-result-status-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let node_run = create_node_run(&db).await;
    let command = action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    let succeeded = WorkflowResultEvent::status(
        &command,
        WorkflowStatus::Succeeded,
        Some(runinator_models::json!({ "success": true })),
        Some("done".into()),
    );
    let running = WorkflowResultEvent::status(&command, WorkflowStatus::Running, None, None);

    assert!(db.apply_workflow_result_event(&succeeded).await.unwrap());
    assert!(db.apply_workflow_result_event(&running).await.unwrap());

    let node_run = db
        .fetch_workflow_node_run(node_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(node_run.status, WorkflowStatus::Succeeded);
    assert_eq!(node_run.message.as_deref(), Some("done"));

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn apply_workflow_result_event_discards_status_from_superseded_attempt() {
    let path = std::env::temp_dir().join(format!(
        "runinator-result-attempt-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let node_run = create_node_run(&db).await;
    // the reducer re-dispatched this node run as attempt 2 after the first attempt was written off.
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(2),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    // a very late terminal result from the superseded first attempt must not settle the retry.
    let mut stale_command =
        action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    stale_command.attempt = 1;
    let stale = WorkflowResultEvent::status(
        &stale_command,
        WorkflowStatus::Failed,
        None,
        Some("late".into()),
    );
    assert!(db.apply_workflow_result_event(&stale).await.unwrap());
    let row = db
        .fetch_workflow_node_run(node_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, WorkflowStatus::Running);
    assert_eq!(row.message, None);

    // the current attempt's result applies normally.
    let mut current_command =
        action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    current_command.attempt = 2;
    let current =
        WorkflowResultEvent::status(&current_command, WorkflowStatus::Succeeded, None, None);
    assert!(db.apply_workflow_result_event(&current).await.unwrap());
    let row = db
        .fetch_workflow_node_run(node_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, WorkflowStatus::Succeeded);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn apply_workflow_result_event_applies_legacy_events_without_attempt() {
    let path = std::env::temp_dir().join(format!(
        "runinator-result-legacy-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let node_run = create_node_run(&db).await;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(2),
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    // an event deserialized from an older worker carries attempt 0: applied unconditionally.
    let mut legacy_command =
        action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    legacy_command.attempt = 0;
    let legacy =
        WorkflowResultEvent::status(&legacy_command, WorkflowStatus::Succeeded, None, None);
    assert!(db.apply_workflow_result_event(&legacy).await.unwrap());
    let row = db
        .fetch_workflow_node_run(node_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, WorkflowStatus::Succeeded);

    let _ = fs::remove_file(path);
}
