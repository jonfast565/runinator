use super::*;
use crate::archive::ArchiveTable;
use crate::interfaces::DatabaseImpl;
use chrono::{Duration, Utc};
use runinator_comm::{ActionCommand, WorkflowResultEvent};
use runinator_models::value::Value;
use runinator_models::{
    auth::{ApiKey, ApiKeyRecord, Grant, Permission, PrincipalType, ResourceType},
    notifications::NewNotification,
    orgs::OrgRole,
    runs::NewRunChunk,
    settings::SettingKind,
    workflows::{
        WorkflowAction, WorkflowDefinition, WorkflowGraph, WorkflowNodeRun, WorkflowStatus,
        WorkflowTrigger, WorkflowTriggerKind,
    },
};
use uuid::Uuid;

#[tokio::test]
async fn settings_round_trip_by_kind_scope_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-settings-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // insert a secret and a config that share a scope/name but differ by kind: they must not collide.
    db.upsert_setting(
        SettingKind::Secret,
        "jira".into(),
        "token".into(),
        b"cipher-a".to_vec(),
        100,
    )
    .await
    .unwrap();
    db.upsert_setting(
        SettingKind::Config,
        "jira".into(),
        "token".into(),
        b"cipher-b".to_vec(),
        200,
    )
    .await
    .unwrap();

    let secret = db
        .fetch_setting(SettingKind::Secret, "jira".into(), "token".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(secret.value, b"cipher-a");
    assert_eq!(secret.updated_at, 100);
    assert_eq!(secret.kind, SettingKind::Secret);

    let config = db
        .fetch_setting(SettingKind::Config, "jira".into(), "token".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(config.value, b"cipher-b");

    // upsert replaces value and timestamp in place.
    db.upsert_setting(
        SettingKind::Secret,
        "jira".into(),
        "token".into(),
        b"cipher-c".to_vec(),
        300,
    )
    .await
    .unwrap();
    let updated = db
        .fetch_setting(SettingKind::Secret, "jira".into(), "token".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.value, b"cipher-c");
    assert_eq!(updated.updated_at, 300);

    // list returns both rows; delete is kind-scoped.
    assert_eq!(db.list_settings().await.unwrap().len(), 2);
    db.delete_setting(SettingKind::Secret, "jira".into(), "token".into())
        .await
        .unwrap();
    assert!(
        db.fetch_setting(SettingKind::Secret, "jira".into(), "token".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.fetch_setting(SettingKind::Config, "jira".into(), "token".into())
            .await
            .unwrap()
            .is_some()
    );

    let _ = std::fs::remove_file(path);
}

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
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let newer = db
        .create_workflow_run(
            second,
            second_snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
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
            runinator_models::json!({}),
            runinator_models::json!({}),
            Some("Ticket Work: ITP-123".into()),
            Default::default(),
        )
        .await
        .unwrap();
    let terminal = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            Some("Ticket Work: ITP-123".into()),
            Default::default(),
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
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
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
            configuration: runinator_models::json!({
                "cron": "*/5 * * * * *",
                "parameters": { "source": "cron" }
            }),
            next_execution: Some(due_at),
            blackout_start: None,
            blackout_end: None,
            metadata: runinator_models::json!({ "name": "test-trigger" }),
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
    updated.version = runinator_models::semver::SemVer::new(2, 0, 0);
    updated.definition = WorkflowGraph::from_value(
        runinator_models::json!({ "nodes": [{ "id": "done", "kind": "end" }] }),
    )
    .unwrap();

    let second = db.upsert_workflow(&updated).await.unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(second.id, first.id);
    assert_eq!(
        second.version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    assert_eq!(second.definition, updated.definition);
    assert_eq!(workflows.len(), 1);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn namespaced_workflow_persists_and_resolves_by_qualified_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-namespace-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // two workflows share the bare name "ticket_work" but live in different namespaces.
    let mut core = workflow("ticket_work");
    core.namespace = Some("core_sdlc".into());
    let mut ops = workflow("ticket_work");
    ops.namespace = Some("ops".into());
    let core = db.upsert_workflow(&core).await.unwrap();
    let ops = db.upsert_workflow(&ops).await.unwrap();
    // distinct namespaces keep them apart rather than colliding on the shared name.
    assert_ne!(core.id, ops.id);
    assert_eq!(core.namespace.as_deref(), Some("core_sdlc"));

    // a qualified subflow target resolves to the matching namespace.
    let resolved = db
        .fetch_workflow_by_name("core_sdlc.ticket_work".into())
        .await
        .unwrap()
        .expect("qualified resolution");
    assert_eq!(resolved.id, core.id);
    assert_eq!(resolved.namespace.as_deref(), Some("core_sdlc"));

    // re-upsert by (namespace, name) identity updates in place, not creating a sibling.
    let again = db.upsert_workflow(&core).await.unwrap();
    assert_eq!(again.id, core.id);
    assert_eq!(db.fetch_workflows().await.unwrap().len(), 2);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn insert_workflow_creates_sibling_row_sharing_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-insert-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let first = db.upsert_workflow(&workflow("pipeline")).await.unwrap();
    let mut copy = workflow("pipeline");
    copy.version = runinator_models::semver::SemVer::new(1, 1, 0);

    let second = db.insert_workflow(&copy).await.unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // a fresh row, not an update of the original.
    assert_ne!(second.id, first.id);
    assert_eq!(second.name, first.name);
    assert_eq!(
        second.version,
        runinator_models::semver::SemVer::new(1, 1, 0)
    );
    assert_eq!(workflows.len(), 2);

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
    let command = action_command(Uuid::new_v4(), Uuid::new_v4(), "node-a");
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

#[tokio::test]
async fn ready_nodes_are_claimed_once_until_lease_expires() {
    let path = std::env::temp_dir().join(format!(
        "runinator-ready-nodes-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let workflow_id = db
        .upsert_workflow(&workflow("ready-node-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let event = runinator_models::orchestration::NewOrchestrationEvent::new(
        run.id,
        Some("start".into()),
        "workflow_run_created",
        runinator_models::json!({}),
    );
    let ready = db
        .enqueue_ready_node(event, "start".into(), Utc::now())
        .await
        .unwrap()
        .expect("ready node should be inserted");

    let first = db
        .claim_ready_nodes(
            "scheduler-a".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].id, ready.id);

    let second = db
        .claim_ready_nodes(
            "scheduler-b".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert!(second.is_empty());

    let reclaimed = db
        .claim_ready_nodes(
            "scheduler-b".into(),
            Utc::now() + Duration::seconds(31),
            Utc::now() + Duration::seconds(60),
            10,
        )
        .await
        .unwrap();
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].claimed_by.as_deref(), Some("scheduler-b"));

    assert!(
        db.complete_ready_node(ready.id, "scheduler-b".into())
            .await
            .unwrap()
    );

    let after_complete = db
        .claim_ready_nodes(
            "scheduler-a".into(),
            Utc::now() + Duration::seconds(61),
            Utc::now() + Duration::seconds(90),
            10,
        )
        .await
        .unwrap();
    assert!(after_complete.is_empty());

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

#[tokio::test]
async fn delete_workflow_cascades_runs_and_execution_records() {
    let path = std::env::temp_dir().join(format!(
        "runinator-delete-cascade-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("cascade-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let node_run = db
        .create_workflow_node_run(run.id, "node-a".into(), runinator_models::json!({}))
        .await
        .unwrap();
    // a chunk result event populates workflow_node_chunks + workflow_result_events.
    let command = action_command(run.id, node_run.id, &node_run.node_id);
    let chunk = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );
    db.apply_workflow_result_event(&chunk).await.unwrap();
    // a ready node populates workflow_orchestration_events + workflow_ready_nodes.
    let event = runinator_models::orchestration::NewOrchestrationEvent::new(
        run.id,
        Some("start".into()),
        "workflow_run_created",
        runinator_models::json!({}),
    );
    db.enqueue_ready_node(event, "start".into(), Utc::now())
        .await
        .unwrap();

    db.delete_workflow(workflow_id).await.unwrap();

    assert!(db.fetch_workflow(workflow_id).await.unwrap().is_none());
    assert!(db.fetch_recent_workflow_runs().await.unwrap().is_empty());
    assert!(
        db.fetch_workflow_node_run(node_run.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.fetch_workflow_node_run_chunks(node_run.id, None, 100)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn waiting_signal_runs_are_routable_by_correlation_key() {
    let path = std::env::temp_dir().join(format!(
        "runinator-signal-correlation-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("signal-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();

    // two runs park a signal node on the same name but different correlation keys.
    let park = |key: &'static str| {
        let db = &db;
        let snapshot = snapshot.clone();
        async move {
            let run = db
                .create_workflow_run(
                    workflow_id,
                    snapshot,
                    runinator_models::json!({}),
                    runinator_models::json!({}),
                    None,
                    Default::default(),
                )
                .await
                .unwrap();
            let node_run = db
                .create_workflow_node_run(run.id, "wait_review".into(), runinator_models::json!({}))
                .await
                .unwrap();
            let state = runinator_models::workflow_state::SignalState {
                name: "github.review".into(),
                correlation_key: Some(key.into()),
            };
            db.update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Waiting,
                Some(1),
                None,
                None,
                Some(serde_json::to_value(&state).unwrap().into()),
                Some("signal_waiting".into()),
                None,
            )
            .await
            .unwrap();
            (run.id, node_run.id)
        }
    };
    let (run_abc, node_abc) = park("ABC-1").await;
    let (_run_xyz, _node_xyz) = park("XYZ-9").await;

    let waiting = db
        .fetch_workflow_node_runs_by_status(WorkflowStatus::Waiting)
        .await
        .unwrap();
    assert_eq!(waiting.len(), 2);

    // the same predicate the ws repository uses must select exactly the matching run.
    let matched: Vec<_> = waiting
        .iter()
        .filter(|run| {
            serde_json::from_value::<runinator_models::workflow_state::SignalState>(
                run.state.clone().into(),
            )
            .map(|state| {
                state.name == "github.review" && state.correlation_key.as_deref() == Some("ABC-1")
            })
            .unwrap_or(false)
        })
        .collect();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].id, node_abc);
    assert_eq!(matched[0].workflow_run_id, run_abc);

    let _ = fs::remove_file(path);
}

#[tokio::test]
async fn executor_lease_is_mutually_exclusive_until_stale_or_released() {
    let path = std::env::temp_dir().join(format!(
        "runinator-executor-lease-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let workflow_id = db
        .upsert_workflow(&workflow("lease-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let node_run = db
        .create_workflow_node_run(run.id, "node-a".into(), runinator_models::json!({}))
        .await
        .unwrap();

    let register = |instance: &'static str| {
        let db = &db;
        async move {
            db.register_replica(
                runinator_models::replicas::ReplicaRegistrationRequest {
                    replica_type: runinator_models::replicas::ReplicaKind::Worker,
                    instance_id: instance.into(),
                    runtime_id: Uuid::new_v4().to_string(),
                    display_name: None,
                    host: None,
                    port: None,
                    base_path: None,
                    version: None,
                    attributes: runinator_models::json!({}),
                },
                None,
            )
            .await
            .unwrap()
            .replica_id
        }
    };
    let worker_a = register("worker-a").await;
    let worker_b = register("worker-b").await;
    let now = Utc::now();
    let stale_before = now - Duration::seconds(300);

    // first claim wins.
    assert!(
        db.claim_workflow_node_run_executor(node_run.id, worker_a, now, stale_before)
            .await
            .unwrap()
    );
    // a concurrent duplicate loses while the lease is fresh.
    assert!(
        !db.claim_workflow_node_run_executor(node_run.id, worker_b, now, stale_before)
            .await
            .unwrap()
    );
    // once the prior claim ages past the cutoff, a retry may steal it.
    let future_cutoff = now + Duration::seconds(1);
    assert!(
        db.claim_workflow_node_run_executor(node_run.id, worker_b, now, future_cutoff)
            .await
            .unwrap()
    );
    // releasing frees the slot for the next attempt immediately.
    db.release_workflow_node_run_executor(node_run.id, worker_b, Utc::now())
        .await
        .unwrap();
    assert!(
        db.claim_workflow_node_run_executor(node_run.id, worker_a, Utc::now(), stale_before)
            .await
            .unwrap()
    );

    let _ = fs::remove_file(path);
}

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(runinator_models::json!({ "nodes": [] })).unwrap(),
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
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    db.create_workflow_node_run(
        workflow_run.id,
        "node-a".into(),
        runinator_models::json!({}),
    )
    .await
    .unwrap()
}

fn action_command(
    workflow_run_id: Uuid,
    workflow_node_run_id: Uuid,
    node_id: &str,
) -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id,
        node_id: node_id.into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: runinator_models::workflows::WorkflowObject::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: Default::default(),
        },
        attempt: 1,
        parameters: runinator_models::json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
    }
}

#[tokio::test]
async fn users_grants_and_teams_round_trip() {
    let path = std::env::temp_dir().join(format!(
        "runinator-authz-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // a user with a local password is resolvable by username, carrying its stored hash.
    let user = db
        .create_user(
            "alice".into(),
            Some("a@x.io".into()),
            false,
            Some("argon-hash".into()),
        )
        .await
        .unwrap();
    let user_id = user.id.unwrap();
    let credential = db
        .fetch_local_credential("alice".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(credential.password_hash, "argon-hash");
    assert_eq!(db.count_users().await.unwrap(), 1);

    // a direct user grant on a workflow is listed for the resource and for the user.
    let workflow_id = Uuid::now_v7();
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::Edit,
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let grants = db
        .list_grants("workflow".into(), workflow_id)
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].permission, Permission::Edit);
    let user_grants = db
        .list_user_grants("workflow".into(), user_id)
        .await
        .unwrap();
    assert_eq!(user_grants.len(), 1);

    // upsert: re-granting the same (resource, principal) updates the permission in place.
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::Own,
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let grants = db
        .list_grants("workflow".into(), workflow_id)
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].permission, Permission::Own);

    // teams: membership feeds team-scoped grants.
    let team = db.create_team("ops".into()).await.unwrap();
    let team_id = team.id.unwrap();
    let updated_team = db.update_team(team_id, "platform".into()).await.unwrap();
    assert_eq!(updated_team.name, "platform");
    db.add_team_member(team_id, user_id).await.unwrap();
    db.add_team_member(team_id, user_id).await.unwrap(); // idempotent
    assert_eq!(db.list_user_team_ids(user_id).await.unwrap(), vec![team_id]);
    assert_eq!(
        db.list_user_teams(user_id).await.unwrap()[0].name,
        "platform"
    );
    assert_eq!(
        db.list_team_members(team_id).await.unwrap()[0].username,
        "alice"
    );
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: PrincipalType::Team,
        principal_id: team_id,
        permission: Permission::Run,
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let team_grants = db
        .list_team_grants("workflow".into(), team_id)
        .await
        .unwrap();
    assert_eq!(team_grants.len(), 1);
    assert_eq!(team_grants[0].permission, Permission::Run);
}

#[tokio::test]
async fn orgs_and_memberships_round_trip() {
    let path = std::env::temp_dir().join(format!(
        "runinator-orgs-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let acme = db.create_org("Acme".into(), "acme".into()).await.unwrap();
    let acme_id = acme.id.unwrap();
    // slug is the unique routing identifier.
    assert!(db.fetch_org_by_slug("acme".into()).await.unwrap().is_some());
    assert_eq!(db.list_orgs().await.unwrap().len(), 1);

    let user = db
        .create_user("bob".into(), None, false, None)
        .await
        .unwrap();
    let user_id = user.id.unwrap();

    // membership is idempotent on (org, user); re-adding updates the role in place.
    db.add_org_member(acme_id, user_id, OrgRole::Member)
        .await
        .unwrap();
    db.add_org_member(acme_id, user_id, OrgRole::Admin)
        .await
        .unwrap();
    let membership = db
        .fetch_org_membership(acme_id, user_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(membership.role, OrgRole::Admin);
    assert_eq!(db.list_org_members(acme_id).await.unwrap().len(), 1);

    // the user's org list carries their role in each org.
    let user_orgs = db.list_user_orgs(user_id).await.unwrap();
    assert_eq!(user_orgs.len(), 1);
    assert_eq!(user_orgs[0].1, OrgRole::Admin);

    // update flips the disabled flag and rename; slug is immutable.
    let updated = db
        .update_org(acme_id, Some("Acme Inc".into()), Some(true))
        .await
        .unwrap();
    assert_eq!(updated.name, "Acme Inc");
    assert!(updated.disabled);
    assert_eq!(updated.slug, "acme");

    // removing the member empties the roster; deleting the org clears everything.
    db.remove_org_member(acme_id, user_id).await.unwrap();
    assert!(db.list_org_members(acme_id).await.unwrap().is_empty());
    db.delete_org(acme_id).await.unwrap();
    assert!(db.fetch_org(acme_id).await.unwrap().is_none());
}

#[tokio::test]
async fn api_keys_support_admin_lookup_update_and_revoke() {
    let path = std::env::temp_dir().join(format!(
        "runinator-api-keys-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let user = db
        .create_user("api-user".into(), None, false, None)
        .await
        .unwrap();
    let user_id = user.id.unwrap();
    let key_id = Uuid::now_v7();
    let expires_at = Utc::now() + Duration::days(1);
    let key = db
        .create_api_key(ApiKeyRecord {
            key: ApiKey {
                id: Some(key_id),
                name: "initial".into(),
                user_id: Some(user_id),
                is_service: false,
                key_prefix: "testprefix".into(),
                last_used_at: None,
                expires_at: Some(expires_at),
                disabled: false,
                created_at: Utc::now(),
            },
            is_admin: false,
            key_hash: "hash".into(),
        })
        .await
        .unwrap();
    assert_eq!(key.id, Some(key_id));

    let fetched = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert_eq!(fetched.key.name, "initial");
    assert_eq!(fetched.key_hash, "hash");

    let updated = db
        .update_api_key(key_id, Some("renamed".into()), Some(None), Some(true))
        .await
        .unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.expires_at, None);
    assert!(updated.disabled);

    db.revoke_api_key(key_id).await.unwrap();
    let revoked = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert!(revoked.key.disabled);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn dead_letters_and_audit_log_round_trip() {
    let path = std::env::temp_dir().join(format!(
        "runinator-dlq-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let event_id = Uuid::now_v7();
    let stored = db
        .record_dead_letter(runinator_models::json!({
            "channel": "result",
            "event_id": event_id.to_string(),
            "dedupe_key": "poison-1",
            "attempts": 3,
            "error": "forced failure",
            "payload": {"kind": "chunk"},
        }))
        .await
        .unwrap();
    assert_eq!(
        stored.get("channel").and_then(Value::as_str),
        Some("result")
    );
    assert_eq!(stored.get("attempts").and_then(Value::as_i64), Some(3));

    // filter by channel, and confirm a non-matching channel returns nothing.
    let all = db.fetch_dead_letters(None, 50).await.unwrap();
    assert_eq!(all.len(), 1);
    let filtered = db
        .fetch_dead_letters(Some("ingress".into()), 50)
        .await
        .unwrap();
    assert!(filtered.is_empty());
    assert_eq!(
        all[0]
            .get("payload")
            .and_then(|p| p.get("kind"))
            .and_then(Value::as_str),
        Some("chunk")
    );

    let actor = Uuid::now_v7();
    db.record_audit_log(runinator_models::json!({
        "actor_id": actor.to_string(),
        "actor_kind": "user",
        "action": "auth.login",
        "outcome": "success",
        "detail": "ok",
    }))
    .await
    .unwrap();
    db.record_audit_log(runinator_models::json!({
        "actor_kind": "anonymous",
        "action": "auth.login",
        "outcome": "failure",
        "detail": "bad password",
    }))
    .await
    .unwrap();

    let by_actor = db.fetch_audit_log(Some(actor), None, 50).await.unwrap();
    assert_eq!(by_actor.len(), 1);
    assert_eq!(
        by_actor[0].get("outcome").and_then(Value::as_str),
        Some("success")
    );
    let by_action = db
        .fetch_audit_log(None, Some("auth.login".into()), 50)
        .await
        .unwrap();
    assert_eq!(by_action.len(), 2);

    let _ = std::fs::remove_file(path);
}

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
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE dead_letters SET created_at = ? WHERE id = ?")
        .bind(recent_timestamp)
        .bind(recent_id)
        .execute(&db.pool)
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
    assert_eq!(
        rows[0]
            .row
            .get("payload")
            .and_then(Value::as_str)
            .unwrap()
            .contains("old"),
        true
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
            .execute(&db.pool)
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

#[tokio::test]
async fn jwt_secret_is_encrypted_at_rest_and_round_trips() {
    let path = std::env::temp_dir().join(format!(
        "runinator-jwt-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // generate and persist a fresh signing secret.
    let secret = crate::ensure_jwt_secret(&db, None).await.unwrap();
    assert_eq!(secret.len(), 48);

    // the stored bytes must be sealed (carry the aead header), never the raw secret.
    let stored = db
        .fetch_setting(SettingKind::Secret, "auth".into(), "jwt_secret".into())
        .await
        .unwrap()
        .unwrap()
        .value;
    assert!(
        runinator_utilities::secret_cipher::SecretCipher::is_sealed(&stored),
        "jwt secret must be encrypted at rest"
    );
    assert_ne!(
        stored, secret,
        "stored value must not equal the plaintext secret"
    );

    // loading transparently decrypts back to the same plaintext.
    let loaded = crate::load_jwt_secret(&db).await.unwrap();
    assert_eq!(loaded, secret);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn legacy_plaintext_jwt_secret_loads_and_migrates_to_encrypted() {
    let path = std::env::temp_dir().join(format!(
        "runinator-jwt-legacy-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // simulate a deployment that stored the secret in the clear before encryption was applied.
    let legacy = b"legacy-plaintext-jwt-secret".to_vec();
    db.upsert_setting(
        SettingKind::Secret,
        "auth".into(),
        "jwt_secret".into(),
        legacy.clone(),
        100,
    )
    .await
    .unwrap();

    // a headerless value loads as-is, without being corrupted by the legacy xor path.
    assert_eq!(crate::load_jwt_secret(&db).await.unwrap(), legacy);

    // bootstrap migrates it to the encrypted-at-rest scheme while preserving the value.
    assert_eq!(crate::ensure_jwt_secret(&db, None).await.unwrap(), legacy);
    let migrated = db
        .fetch_setting(SettingKind::Secret, "auth".into(), "jwt_secret".into())
        .await
        .unwrap()
        .unwrap()
        .value;
    assert!(
        runinator_utilities::secret_cipher::SecretCipher::is_sealed(&migrated),
        "legacy secret must be sealed after migration"
    );
    assert_eq!(crate::load_jwt_secret(&db).await.unwrap(), legacy);

    let _ = std::fs::remove_file(path);
}
