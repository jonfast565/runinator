//! the assertions every sql backend must satisfy, written once and run against all three.
//!
//! the method bodies in `crate::operations` are generic over `SqlBackend`, but they are not
//! dialect-free: upserts, claims, and read-backs each branch on `SqlDialect`, and the three
//! migration sets are hand-maintained siblings. those branches are exactly what a sqlite-only suite
//! cannot reach — a mysql `ON DUPLICATE KEY` that silently updates the wrong row, or a postgres
//! `RETURNING` that a mysql path has to emulate with a second SELECT, both pass sqlite untouched.
//!
//! so this body is the parity contract: `sqlite_lifecycle` runs it unconditionally, and
//! `mariadb_full_lifecycle` / `postgres_full_lifecycle` run the identical body against a live
//! engine when their url is set. running it on sqlite is what keeps it from rotting in a workspace
//! where nobody has docker up.

use chrono::{Duration, Utc};
use runinator_comm::{ActionCommand, WorkflowResultEvent, WorkflowResultEventKind};
use runinator_models::{
    revisions::{RevisionSource, WorkflowRevision},
    runs::RunStatus,
    settings::SettingKind,
    types::RuninatorType,
    value::Value,
    workflows::{
        WorkflowAction, WorkflowDefinition, WorkflowGraph, WorkflowObject, WorkflowStatus,
        WorkflowTrigger, WorkflowTriggerKind,
    },
};
use uuid::Uuid;

// `DatabaseImpl` composes every role trait, so bounding on it brings all of their methods into
// scope without importing the roles one by one.
use crate::interfaces::DatabaseImpl;

fn sample_workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        namespace: None,
        org_id: None,
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
            idempotency_key: None,
        },
        attempt: 1,
        parameters: runinator_models::json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        idempotency_key: None,
    }
}

/// run the full cross-dialect lifecycle against an already-migrated, empty store.
///
/// the store must be exclusive to this call: several assertions count rows or depend on a claim
/// finding nothing else outstanding.
pub(crate) async fn assert_dialect_parity<T: DatabaseImpl>(db: &T) {
    assert_workflow_upsert(db).await;
    let after = db.fetch_workflows().await.unwrap().remove(0);
    let id = after.id.expect("the upserted workflow has an id");

    assert_revision_history(db, &after).await;
    assert_trigger_upsert(db, id).await;
    let (run_id, node_id) = assert_run_claim_and_results(db, &after).await;
    assert_idempotency_keys(db).await;
    assert_action_dispatch(db, run_id, node_id).await;
    assert_notifications(db).await;
    assert_settings(db).await;
    assert_catalog_upsert(db).await;
    assert_automation_records(db, run_id).await;

    // the legacy run mapper reads a column named `trigger`, which is reserved in mysql and has to
    // be quoted per dialect; an unquoted build fails here rather than in production.
    assert!(
        db.fetch_runs_by_status(RunStatus::Running)
            .await
            .unwrap()
            .is_empty()
    );
}

// upsert has three entry paths and each dialect renders them differently: insert, update by id,
// and match-by-name with no id. the last one is the one that duplicates rows when a dialect's
// conflict target is wrong, so it is asserted by row count and not just by returned id.
async fn assert_workflow_upsert<T: DatabaseImpl>(db: &T) {
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

    let by_name = db.upsert_workflow(&sample_workflow("alpha")).await.unwrap();
    assert_eq!(by_name.id, Some(id));
    assert_eq!(db.fetch_workflows().await.unwrap().len(), 1);
}

// revision sequencing and the unchanged-save dedupe both read the head row back before inserting,
// under a unique index on (workflow_id, revision). that read-then-insert is the shape most likely
// to differ between dialects, and getting it wrong either forks history or stops recording it.
async fn assert_revision_history<T: DatabaseImpl>(db: &T, workflow: &WorkflowDefinition) {
    let id = workflow.id.expect("workflow has an id");
    let mut revision = WorkflowRevision {
        id: Uuid::nil(),
        workflow_id: id,
        revision: 0,
        version: workflow.version,
        name: workflow.name.clone(),
        input_type: workflow.input_type.clone(),
        definition: workflow.definition.clone(),
        source: RevisionSource::Pack,
        actor_id: None,
        actor_kind: "system".to_string(),
        note: None,
        created_at: None,
    };

    let first = db
        .insert_workflow_revision(&revision)
        .await
        .unwrap()
        .expect("first revision recorded");
    assert_eq!(first.revision, 1);
    assert!(
        db.insert_workflow_revision(&revision)
            .await
            .unwrap()
            .is_none(),
        "an identical save must not grow history"
    );

    revision.name = "alpha-renamed".to_string();
    assert_eq!(
        db.insert_workflow_revision(&revision)
            .await
            .unwrap()
            .expect("changed revision recorded")
            .revision,
        2
    );
    assert_eq!(db.fetch_workflow_revisions(id, 50).await.unwrap().len(), 2);
    assert_eq!(
        db.fetch_workflow_revision(id, 1)
            .await
            .unwrap()
            .expect("revision 1")
            .name,
        "alpha"
    );
}

async fn assert_trigger_upsert<T: DatabaseImpl>(db: &T, workflow_id: Uuid) {
    let saved = db
        .upsert_workflow_trigger(&sample_trigger(workflow_id))
        .await
        .unwrap();
    let trigger_id = saved.id.expect("trigger insert assigns an id");

    let mut retrig = saved.clone();
    retrig.enabled = false;
    let retrigged = db.upsert_workflow_trigger(&retrig).await.unwrap();
    assert_eq!(retrigged.id, Some(trigger_id));
    assert!(!retrigged.enabled);
}

// the scheduler claim is a multi-row conditional UPDATE plus a read of the rows it took. dialects
// without RETURNING emulate it with a marker column and a follow-up SELECT, so a broken lease
// shows up here as a second scheduler claiming work the first already holds.
async fn assert_run_claim_and_results<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> (Uuid, Uuid) {
    let id = workflow.id.expect("workflow has an id");
    let run = db
        .create_workflow_run(
            id,
            workflow.clone(),
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

    // result events are deduped by an insert that must not error on a replay. dialects spell that
    // three ways (INSERT IGNORE / ON CONFLICT DO NOTHING / INSERT OR IGNORE) and the boolean they
    // return is what stops a redelivered worker result from being applied twice.
    let node = db
        .create_workflow_node_run(run.id, "task-1".into(), Value::Null, None)
        .await
        .unwrap();
    let event = WorkflowResultEvent {
        event_id: Uuid::new_v4(),
        command_id: Uuid::new_v4(),
        workflow_run_id: run.id,
        workflow_node_run_id: node.id,
        node_id: "task-1".into(),
        attempt: 1,
        timestamp: Utc::now(),
        kind: WorkflowResultEventKind::Status {
            status: WorkflowStatus::Succeeded,
            output_json: None,
            message: None,
        },
        trace_id: Uuid::nil(),
        notification_delivery_id: None,
    };
    assert!(
        db.apply_workflow_result_event(&event).await.unwrap(),
        "first apply succeeds"
    );
    assert!(
        !db.apply_workflow_result_event(&event).await.unwrap(),
        "replay is ignored"
    );

    (run.id, node.id)
}

// `key` is reserved in mysql, so this exercises identifier quoting as well as first-writer-wins:
// a second write returning the second value would make a retried request non-idempotent.
async fn assert_idempotency_keys<T: DatabaseImpl>(db: &T) {
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

    let fetched = db.fetch_idempotency_key(scope, key).await.unwrap().unwrap();
    assert_eq!(
        fetched
            .get("result")
            .and_then(|r| r.get("v"))
            .and_then(Value::as_i64),
        Some(1),
        "first writer wins"
    );
}

async fn assert_action_dispatch<T: DatabaseImpl>(db: &T, run_id: Uuid, node_id: Uuid) {
    let first = db
        .enqueue_action_dispatch("dedupe-1".into(), sample_action(run_id, node_id))
        .await
        .unwrap();
    let again = db
        .enqueue_action_dispatch("dedupe-1".into(), sample_action(run_id, node_id))
        .await
        .unwrap();
    assert_eq!(first.id, again.id, "dedupe key returns the same row");

    let now = Utc::now();
    let claimed = db
        .claim_pending_action_dispatches("pub-a".into(), now, now + Duration::seconds(30), 10)
        .await
        .unwrap();
    assert!(claimed.iter().any(|d| d.id == first.id));
}

async fn assert_notifications<T: DatabaseImpl>(db: &T) {
    let note = db.create_notification(&Default::default()).await.unwrap();
    let read = db.mark_notification_read(note.id).await.unwrap().unwrap();
    assert!(read.read_at.is_some());
    assert!(
        db.mark_notification_read(Uuid::nil())
            .await
            .unwrap()
            .is_none(),
        "a missing id returns None rather than erroring on the read-back"
    );
}

// settings round trip a binary value (sealed secrets) through a composite-primary-key upsert; a
// dialect storing it as text would corrupt every credential it holds.
async fn assert_settings<T: DatabaseImpl>(db: &T) {
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
}

async fn assert_catalog_upsert<T: DatabaseImpl>(db: &T) {
    let item = runinator_models::json!({ "uri": "cat://x", "item_type": "t", "name": "n", "version": "1" });
    db.upsert_catalog_item(item).await.unwrap();
    let revised = runinator_models::json!({ "uri": "cat://x", "item_type": "t2", "name": "n", "version": "1" });
    let upserted = db.upsert_catalog_item(revised).await.unwrap();
    assert_eq!(
        upserted.get("item_type").and_then(Value::as_str),
        Some("t2"),
        "the uri unique key must update in place"
    );
}

// an insert has to read its row back, and an update has to do so even when the write changed
// nothing: mysql reports zero affected rows for a no-op update, so a caller keying off the row
// count would return nothing for an unchanged record.
async fn assert_automation_records<T: DatabaseImpl>(db: &T, run_id: Uuid) {
    let automation = runinator_models::json!({
        "provider": "github",
        "resource_type": "pull_request",
        "external_id": "42",
        "status": "open",
        "title": "Initial title",
        "workflow_run_id": run_id,
        "node_id": "task-1",
        "metadata": { "source": "parity-test" }
    });
    let created = db
        .create_automation_record("review".into(), automation.clone())
        .await
        .unwrap();
    let record_id = created
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
        .expect("automation record insert assigns an id");
    assert_eq!(
        created.get("title").and_then(Value::as_str),
        Some("Initial title")
    );

    let unchanged = db
        .update_automation_record("review".into(), record_id, automation)
        .await
        .unwrap();
    assert_eq!(
        unchanged.get("title").and_then(Value::as_str),
        Some("Initial title"),
        "a no-op update must still return the record"
    );

    let revised = runinator_models::json!({
        "provider": "github",
        "resource_type": "pull_request",
        "external_id": "42",
        "status": "resolved",
        "title": "Updated title",
        "workflow_run_id": run_id,
        "node_id": "task-1",
        "metadata": { "source": "parity-test", "updated": true }
    });
    let changed = db
        .update_automation_record("review".into(), record_id, revised)
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
}
