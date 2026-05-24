use super::*;
use crate::interfaces::DatabaseImpl;
use runinator_comm::{ActionCommand, WorkflowResultEvent};
use runinator_models::{
    runs::NewRunChunk,
    workflows::{WorkflowAction, WorkflowStatus},
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

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        version: 1,
        enabled: true,
        input_schema: serde_json::json!({}),
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
