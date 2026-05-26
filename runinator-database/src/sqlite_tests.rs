use super::*;
use crate::interfaces::DatabaseImpl;
use chrono::Duration;
use runinator_comm::{ActionCommand, WorkflowResultEvent};
use runinator_models::{
    runs::NewRunChunk,
    workflows::{
        WorkflowAction, WorkflowDefinition, WorkflowStatus, WorkflowTrigger, WorkflowTriggerKind,
    },
};
use uuid::Uuid;

#[tokio::test]
async fn fetch_recent_workflow_runs_returns_all_workflows_newest_first() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-runs-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let first = db
        .upsert_workflow(&workflow("first"))
        .await
        .unwrap()
        .id
        .unwrap();
    let second = db
        .upsert_workflow(&workflow("second"))
        .await
        .unwrap()
        .id
        .unwrap();
    let first_snapshot = db.fetch_workflow(first).await.unwrap().unwrap();
    let second_snapshot = db.fetch_workflow(second).await.unwrap().unwrap();
    let older = db
        .create_workflow_run(
            first,
            first_snapshot,
            serde_json::json!({}),
            serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    let newer = db
        .create_workflow_run(
            second,
            second_snapshot,
            serde_json::json!({}),
            serde_json::json!({}),
            None,
        )
        .await
        .unwrap();

    let runs = db.fetch_recent_workflow_runs().await.unwrap();
    assert_eq!(
        runs.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![newer.id, older.id]
    );
    assert_eq!(
        runs.iter().map(|run| run.workflow_id).collect::<Vec<_>>(),
        vec![second, first]
    );
    assert_eq!(
        runs[0]
            .workflow_snapshot
            .as_ref()
            .map(|workflow| workflow.name.as_str()),
        Some("second")
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn workflow_runs_can_be_created_and_queried_by_open_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-runs-by-name-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("ticket work"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let open = db
        .create_workflow_run(
            workflow_id,
            snapshot.clone(),
            serde_json::json!({}),
            serde_json::json!({}),
            Some("Ticket Work: ITP-123".into()),
        )
        .await
        .unwrap();
    let terminal = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            serde_json::json!({}),
            serde_json::json!({}),
            Some("Ticket Work: ITP-123".into()),
        )
        .await
        .unwrap();
    db.update_workflow_run_status(terminal.id, WorkflowStatus::Succeeded, None, None, None)
        .await
        .unwrap();

    let all = db
        .fetch_workflow_runs_by_name("Ticket Work: ITP-123".into(), false)
        .await
        .unwrap();
    let open_only = db
        .fetch_workflow_runs_by_name("Ticket Work: ITP-123".into(), true)
        .await
        .unwrap();

    assert_eq!(
        all.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![terminal.id, open.id]
    );
    assert_eq!(
        open_only.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![open.id]
    );
    assert_eq!(open.name.as_deref(), Some("Ticket Work: ITP-123"));

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn scheduler_claims_open_workflow_runs_once_until_lease_expires() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-claims-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("claim-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            serde_json::json!({}),
            serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    let now = Utc::now();

    let first = db
        .claim_workflow_runs_for_scheduler(
            "scheduler-a".into(),
            vec![WorkflowStatus::Queued],
            now,
            now + Duration::seconds(60),
            10,
        )
        .await
        .unwrap();
    let second = db
        .claim_workflow_runs_for_scheduler(
            "scheduler-b".into(),
            vec![WorkflowStatus::Queued],
            now,
            now + Duration::seconds(60),
            10,
        )
        .await
        .unwrap();
    let expired = db
        .claim_workflow_runs_for_scheduler(
            "scheduler-b".into(),
            vec![WorkflowStatus::Queued],
            now + Duration::seconds(61),
            now + Duration::seconds(120),
            10,
        )
        .await
        .unwrap();

    assert_eq!(
        first.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![run.id]
    );
    assert!(second.is_empty());
    assert_eq!(
        expired.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![run.id]
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn due_trigger_firing_is_idempotent_and_advances_next_execution() {
    let path = std::env::temp_dir().join(format!(
        "runinator-trigger-firing-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("trigger-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let due_at = Utc::now() - Duration::seconds(60);
    let trigger = db
        .upsert_workflow_trigger(&WorkflowTrigger {
            id: None,
            workflow_id,
            kind: WorkflowTriggerKind::Cron,
            enabled: true,
            configuration: serde_json::json!({
                "cron": "*/5 * * * * *",
                "parameters": { "source": "cron" }
            }),
            next_execution: Some(due_at),
            blackout_start: None,
            blackout_end: None,
            metadata: serde_json::json!({ "name": "test-trigger" }),
            created_at: None,
            updated_at: None,
        })
        .await
        .unwrap();

    let first = db
        .claim_due_workflow_trigger_firings("scheduler-a".into(), Utc::now(), 10)
        .await
        .unwrap();
    db.update_workflow_trigger_next_execution(trigger.id.unwrap(), Some(due_at))
        .await
        .unwrap();
    let duplicate = db
        .claim_due_workflow_trigger_firings("scheduler-b".into(), Utc::now(), 10)
        .await
        .unwrap();
    let refreshed = db
        .fetch_workflow_trigger(trigger.id.unwrap())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(first.len(), 1);
    assert_eq!(first[0].parameters["source"], "cron");
    assert!(duplicate.is_empty());
    assert!(refreshed.next_execution.is_some());

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn upsert_workflow_without_id_updates_existing_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-upsert-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let first = db.upsert_workflow(&workflow("pipeline")).await.unwrap();
    let mut updated = workflow("pipeline");
    updated.version = 2;
    updated.definition = serde_json::json!({ "nodes": [{ "id": "done", "kind": "end" }] });

    let second = db.upsert_workflow(&updated).await.unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(second.id, first.id);
    assert_eq!(second.version, 2);
    assert_eq!(second.definition, updated.definition);
    assert_eq!(workflows.len(), 1);

    let _ = fs::remove_file(path);
}

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
        Some(serde_json::json!({ "success": true })),
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
async fn action_dispatch_outbox_is_idempotent_and_tracks_publish_state() {
    let path = std::env::temp_dir().join(format!(
        "runinator-action-dispatches-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let command = action_command(42, 99, "node-a");

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
    let command = action_command(42, 99, "node-a");
    let dispatch = db
        .enqueue_action_dispatch("dispatch-key".into(), command)
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_action_dispatches SET command_json = ? WHERE id = ?")
        .bind("{")
        .bind(dispatch.id)
        .execute(&db.pool)
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

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        version: 1,
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: serde_json::json!({ "nodes": [] }),
        created_at: None,
        updated_at: None,
    }
}

async fn create_node_run(db: &SqliteDb) -> WorkflowNodeRun {
    let workflow_id = db
        .upsert_workflow(&workflow("result-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let workflow_run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            serde_json::json!({}),
            serde_json::json!({}),
            None,
        )
        .await
        .unwrap();
    db.create_workflow_node_run(workflow_run.id, "node-a".into(), serde_json::json!({}))
        .await
        .unwrap()
}

fn action_command(workflow_run_id: i64, workflow_node_run_id: i64, node_id: &str) -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id,
        node_id: node_id.into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: serde_json::json!({}),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: serde_json::json!({}),
    }
}
