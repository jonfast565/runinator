// integration tests against a live MariaDB/MySQL, gated on RUNINATOR_TEST_MYSQL_URL
// (e.g. mysql://root:runinator@127.0.0.1:3399/runinator). they exercise the dialect branches that
// the in-memory sqlite suite cannot: ON DUPLICATE KEY upserts, UPDATE+SELECT claims, INSERT IGNORE,
// and reserved-word quoting. each run provisions a throwaway database so it is independent.

use super::*;
use crate::interfaces::DatabaseImpl;
use chrono::{Duration, Utc};
use runinator_comm::{ActionCommand, WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::{
    runs::RunStatus,
    settings::SettingKind,
    types::RuninatorType,
    value::Value,
    workflows::{
        WorkflowAction, WorkflowDefinition, WorkflowGraph, WorkflowObject, WorkflowStatus,
        WorkflowTrigger, WorkflowTriggerKind,
    },
};
use sqlx::{Connection, MySqlConnection};
use uuid::Uuid;

fn base_url() -> Option<String> {
    std::env::var("RUNINATOR_TEST_MYSQL_URL").ok()
}

// split a `.../dbname` url into (server-url-without-db, dbname).
fn split_url(url: &str) -> (String, String) {
    let (server, db) = url
        .rsplit_once('/')
        .expect("url must contain a database path");
    (server.to_string(), db.to_string())
}

async fn fresh_db() -> Option<(MySqlDb, String, String)> {
    let url = base_url()?;
    let (server, _) = split_url(&url);
    let db = format!("runinator_test_{}", Uuid::new_v4().simple());
    let mut conn = MySqlConnection::connect(&server).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {db}"))
        .execute(&mut conn)
        .await
        .unwrap();
    let db_url = format!("{server}/{db}");
    let pool = MySqlDb::new(&db_url).await.unwrap();
    pool.run_init_scripts(&Vec::new()).await.unwrap();
    Some((pool, server, db))
}

async fn drop_db(server: &str, db: &str) {
    let mut conn = MySqlConnection::connect(server).await.unwrap();
    sqlx::query(&format!("DROP DATABASE IF EXISTS {db}"))
        .execute(&mut conn)
        .await
        .unwrap();
}

fn sample_workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        namespace: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(runinator_models::json!({ "nodes": [] })).unwrap(),
        created_at: None,
        updated_at: None,
    }
}

fn sample_trigger(workflow_id: Uuid) -> WorkflowTrigger {
    WorkflowTrigger {
        id: None,
        workflow_id,
        kind: WorkflowTriggerKind::Cron,
        enabled: true,
        configuration: runinator_models::json!({ "cron": "0 0 * * *" }),
        next_execution: None,
        blackout_start: None,
        blackout_end: None,
        metadata: runinator_models::json!({}),
        created_at: None,
        updated_at: None,
    }
}

fn sample_action(workflow_run_id: Uuid, workflow_node_run_id: Uuid) -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id,
        node_id: "task-1".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: WorkflowObject::default(),
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
async fn mariadb_full_lifecycle() {
    let Some((db, server, dbname)) = fresh_db().await else {
        eprintln!("skipping: set RUNINATOR_TEST_MYSQL_URL to run MariaDB tests");
        return;
    };

    // workflow upsert: first by name (insert), then by id (ON DUPLICATE KEY UPDATE + select-back).
    let created = db.upsert_workflow(&sample_workflow("alpha")).await.unwrap();
    let id = created.id.expect("insert assigns an id");
    let mut updated = sample_workflow("alpha");
    updated.id = Some(id);
    updated.version = runinator_models::semver::SemVer::new(2, 0, 0);
    let after = db.upsert_workflow(&updated).await.unwrap();
    assert_eq!(after.id, Some(id));
    assert_eq!(
        after.version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    // upsert by name with no id must find and update the existing row, not duplicate it.
    let by_name = db.upsert_workflow(&sample_workflow("alpha")).await.unwrap();
    assert_eq!(by_name.id, Some(id));
    assert_eq!(db.fetch_workflows().await.unwrap().len(), 1);

    // trigger upsert (insert then update by id).
    let saved = db
        .upsert_workflow_trigger(&sample_trigger(id))
        .await
        .unwrap();
    let trigger_id = saved.id.expect("trigger insert assigns an id");
    let mut retrig = saved.clone();
    retrig.enabled = false;
    let retrigged = db.upsert_workflow_trigger(&retrig).await.unwrap();
    assert_eq!(retrigged.id, Some(trigger_id));
    assert!(!retrigged.enabled);

    // workflow run + multi-row scheduler claim (derived-table UPDATE then select-back).
    let run = db
        .create_workflow_run(
            id,
            after.clone(),
            Value::Null,
            Value::Null,
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let now = Utc::now();
    let claimed = db
        .claim_workflow_runs_for_scheduler(
            "sched-a".into(),
            vec![WorkflowStatus::Queued],
            now,
            now + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert!(
        claimed.iter().any(|r| r.id == run.id),
        "claim must return the queued run"
    );
    // a second claim under the live lease returns nothing new.
    let again = db
        .claim_workflow_runs_for_scheduler(
            "sched-b".into(),
            vec![WorkflowStatus::Queued],
            now,
            now + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert!(again.is_empty(), "claim must respect an unexpired lease");

    // node run + idempotent result event (INSERT IGNORE on the dedupe table).
    let node = db
        .create_workflow_node_run(run.id, "task-1".into(), Value::Null)
        .await
        .unwrap();
    let event = WorkflowResultEvent {
        event_id: Uuid::new_v4(),
        command_id: Uuid::new_v4(),
        workflow_run_id: run.id,
        workflow_node_run_id: node.id,
        node_id: "task-1".into(),
        timestamp: Utc::now(),
        kind: WorkflowResultEventKind::Status {
            status: WorkflowStatus::Succeeded,
            output_json: None,
            message: None,
        },
        trace_id: Uuid::nil(),
    };
    assert!(
        db.apply_workflow_result_event(&event).await.unwrap(),
        "first apply succeeds"
    );
    assert!(
        !db.apply_workflow_result_event(&event).await.unwrap(),
        "replay is ignored"
    );

    // idempotency keys exercise the reserved-word `key` quoting + ON DUPLICATE KEY first-writer-wins.
    let scope = "scope-x".to_string();
    let key = "key-y".to_string();
    db.put_idempotency_key(
        scope.clone(),
        key.clone(),
        runinator_models::json!({"v": 1}),
    )
    .await
    .unwrap();
    db.put_idempotency_key(
        scope.clone(),
        key.clone(),
        runinator_models::json!({"v": 2}),
    )
    .await
    .unwrap();
    let fetched = db
        .fetch_idempotency_key(scope.clone(), key.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched
            .get("result")
            .and_then(|r| r.get("v"))
            .and_then(Value::as_i64),
        Some(1),
        "first writer wins"
    );

    // action dispatch enqueue (idempotent) + multi-row claim.
    let d1 = db
        .enqueue_action_dispatch("dedupe-1".into(), sample_action(run.id, node.id))
        .await
        .unwrap();
    let d1_again = db
        .enqueue_action_dispatch("dedupe-1".into(), sample_action(run.id, node.id))
        .await
        .unwrap();
    assert_eq!(d1.id, d1_again.id, "dedupe key returns the same row");
    let dispatch_claim = db
        .claim_pending_action_dispatches("pub-a".into(), now, now + Duration::seconds(30), 10)
        .await
        .unwrap();
    assert!(dispatch_claim.iter().any(|d| d.id == d1.id));

    // notifications: create then mark-read (UPDATE then SELECT on mysql).
    let note = db.create_notification(&Default::default()).await.unwrap();
    let read = db.mark_notification_read(note.id).await.unwrap().unwrap();
    assert!(read.read_at.is_some());
    assert!(
        db.mark_notification_read(Uuid::nil())
            .await
            .unwrap()
            .is_none(),
        "missing id returns None"
    );

    // settings round trip (LONGBLOB value, composite PK upsert).
    db.upsert_setting(
        SettingKind::Secret,
        "jira".into(),
        "token".into(),
        b"cipher".to_vec(),
        100,
    )
    .await
    .unwrap();
    let setting = db
        .fetch_setting(SettingKind::Secret, "jira".into(), "token".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(setting.value, b"cipher".to_vec());

    // catalog upsert on the uri unique key.
    let item = runinator_models::json!({ "uri": "cat://x", "item_type": "t", "name": "n", "version": "1" });
    db.upsert_catalog_item(item.clone()).await.unwrap();
    let item2 = runinator_models::json!({ "uri": "cat://x", "item_type": "t2", "name": "n", "version": "1" });
    let upserted = db.upsert_catalog_item(item2).await.unwrap();
    assert_eq!(
        upserted.get("item_type").and_then(Value::as_str),
        Some("t2")
    );

    // automation records: insert should read back via last_insert_id, and update should still
    // read back the row even when mysql reports zero affected rows for a no-op update.
    let automation = runinator_models::json!({
        "provider": "github",
        "resource_type": "pull_request",
        "external_id": "42",
        "status": "open",
        "title": "Initial title",
        "workflow_run_id": run.id,
        "node_id": "task-1",
        "metadata": { "source": "mysql-test" }
    });
    let created_record = db
        .create_automation_record("review".into(), automation.clone())
        .await
        .unwrap();
    let record_id = created_record
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
        .expect("automation record insert assigns an id");
    assert_eq!(
        created_record.get("title").and_then(Value::as_str),
        Some("Initial title")
    );

    let unchanged = db
        .update_automation_record("review".into(), record_id, automation.clone())
        .await
        .unwrap();
    assert_eq!(
        unchanged.get("title").and_then(Value::as_str),
        Some("Initial title"),
        "no-op mysql update must still return the record"
    );

    let updated_record = runinator_models::json!({
        "provider": "github",
        "resource_type": "pull_request",
        "external_id": "42",
        "status": "resolved",
        "title": "Updated title",
        "workflow_run_id": run.id,
        "node_id": "task-1",
        "metadata": { "source": "mysql-test", "updated": true }
    });
    let changed = db
        .update_automation_record("review".into(), record_id, updated_record)
        .await
        .unwrap();
    assert_eq!(
        changed.get("status").and_then(Value::as_str),
        Some("resolved")
    );
    assert_eq!(
        changed.get("title").and_then(Value::as_str),
        Some("Updated title")
    );

    // legacy run row mapper reads the reserved-word `trigger` column.
    assert!(
        db.fetch_runs_by_status(RunStatus::Running)
            .await
            .unwrap()
            .is_empty()
    );

    drop_db(&server, &dbname).await;
}
