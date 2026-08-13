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

use std::collections::BTreeMap;

use chrono::{Duration, Utc};
use runinator_comm::{
    ActionCommand, AgentDirectiveKind, AgentDirectiveResult, AgentDirectiveState,
    AgentDirectiveStatus, WorkflowResultEvent, WorkflowResultEventKind,
};
use runinator_models::{
    auth::{AgentEnrollmentToken, AgentEnrollmentTokenRecord, ApiKey, ApiKeyRecord, PrincipalKind},
    json,
    orchestration::NewOrchestrationEvent,
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
use runinator_store::workflow_mutex::WorkflowMutexClaim;
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
    assert_run_state_cas(db, run_id).await;
    assert_cursor_scoped_ready_nodes(db, run_id).await;
    assert_cooldown_claim(db).await;
    assert_workflow_mutex_claim(db, &after).await;
    assert_agent_enrollment_lifecycle(db).await;
    assert_agent_directive_lifecycle(db).await;

    // the legacy run mapper reads a column named `trigger`, which is reserved in mysql and has to
    // be quoted per dialect; an unquoted build fails here rather than in production.
    assert!(
        db.fetch_runs_by_status(RunStatus::Running)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn assert_workflow_mutex_claim<T: DatabaseImpl>(db: &T, workflow: &WorkflowDefinition) {
    let snapshot = db
        .fetch_workflow(workflow.id.expect("workflow id"))
        .await
        .unwrap()
        .unwrap();
    let make_contender = |node: &'static str| {
        let snapshot = snapshot.clone();
        async move {
            let run = db
                .create_workflow_run(
                    snapshot.id.expect("workflow id"),
                    snapshot,
                    json!({}),
                    json!({}),
                    None,
                    Default::default(),
                )
                .await
                .unwrap();
            let node_run = db
                .create_workflow_node_run(
                    run.id,
                    node.into(),
                    json!({ "name": "fifo" }),
                    None,
                    None,
                )
                .await
                .unwrap();
            (run, node_run)
        }
    };
    let (left, right) = tokio::join!(make_contender("left"), make_contender("right"));
    let now = Utc::now().timestamp();
    let left_cursor = Uuid::now_v7();
    let right_cursor = Uuid::now_v7();
    let claim = |run_id, node_run_id, cursor_id, node_id: &str| WorkflowMutexClaim {
        name: "parity-fifo".into(),
        workflow_run_id: run_id,
        workflow_node_run_id: node_run_id,
        cursor_id,
        node_id: node_id.into(),
        hold_deadline_unix: Some(now - 1),
        enqueued_at_unix: now,
    };
    let left_claim = claim(left.0.id, left.1.id, left_cursor, "left");
    let right_claim = claim(right.0.id, right.1.id, right_cursor, "right");
    let (left_result, right_result) = tokio::join!(
        db.claim_workflow_mutex(left_claim.clone(), now),
        db.claim_workflow_mutex(right_claim.clone(), now),
    );
    let left_result = left_result.unwrap();
    let right_result = right_result.unwrap();
    assert_ne!(
        left_result.acquired, right_result.acquired,
        "exactly one concurrent mutex claimant wins"
    );
    assert!(
        if left_result.acquired {
            left_result.holder_overdue
        } else {
            right_result.holder_overdue
        },
        "an already-expired legacy-style holder is preserved and marked overdue"
    );
    let (winner, loser) = if left_result.acquired {
        (left_claim, right_claim)
    } else {
        (right_claim, left_claim)
    };
    let wake = db
        .release_workflow_mutex(
            winner.name.clone(),
            winner.workflow_run_id,
            winner.cursor_id,
            now + 1,
        )
        .await
        .unwrap()
        .expect("release wakes the fifo successor");
    assert_eq!(wake.workflow_node_run_id, loser.workflow_node_run_id);
    assert!(
        db.claim_workflow_mutex(loser.clone(), now + 1)
            .await
            .unwrap()
            .acquired
    );

    let reentrant_node = db
        .create_workflow_node_run(
            loser.workflow_run_id,
            "reentrant".into(),
            json!({ "name": "fifo" }),
            None,
            None,
        )
        .await
        .unwrap();
    let mut reentrant = loser.clone();
    reentrant.workflow_node_run_id = reentrant_node.id;
    reentrant.node_id = "reentrant".into();
    assert!(
        db.claim_workflow_mutex(reentrant, now + 2)
            .await
            .unwrap()
            .acquired,
        "the owning cursor is reentrant"
    );

    let sibling_node = db
        .create_workflow_node_run(
            loser.workflow_run_id,
            "sibling".into(),
            json!({ "name": "fifo" }),
            None,
            None,
        )
        .await
        .unwrap();
    let sibling = WorkflowMutexClaim {
        workflow_node_run_id: sibling_node.id,
        cursor_id: Uuid::now_v7(),
        node_id: "sibling".into(),
        enqueued_at_unix: now + 2,
        ..loser.clone()
    };
    let blocked = db
        .claim_workflow_mutex(sibling.clone(), now + 2)
        .await
        .unwrap();
    assert!(!blocked.acquired, "a sibling cursor cannot share the mutex");
    assert!(
        blocked.holder_overdue,
        "expiry is reported but does not displace an active holder"
    );
    db.remove_workflow_mutex_waiter(sibling.workflow_node_run_id)
        .await
        .unwrap();

    let successor_node = db
        .create_workflow_node_run(
            winner.workflow_run_id,
            "successor".into(),
            json!({ "name": "fifo" }),
            None,
            None,
        )
        .await
        .unwrap();
    let successor = WorkflowMutexClaim {
        workflow_run_id: winner.workflow_run_id,
        workflow_node_run_id: successor_node.id,
        cursor_id: Uuid::now_v7(),
        node_id: "successor".into(),
        enqueued_at_unix: now + 3,
        ..loser.clone()
    };
    assert!(
        !db.claim_workflow_mutex(successor.clone(), now + 2)
            .await
            .unwrap()
            .acquired
    );

    db.update_workflow_run_status(
        loser.workflow_run_id,
        WorkflowStatus::Canceled,
        None,
        None,
        Some("mutex parity cancellation".into()),
    )
    .await
    .unwrap();
    assert!(
        db.claim_workflow_mutex(successor, now + 3)
            .await
            .unwrap()
            .acquired,
        "a terminal holder is reclaimed by the oldest waiter"
    );
}

async fn assert_agent_directive_lifecycle<T: DatabaseImpl>(db: &T) {
    let replica = db
        .register_replica(
            runinator_models::replicas::ReplicaRegistrationRequest {
                replica_type: runinator_models::replicas::ReplicaKind::Worker,
                instance_id: "parity-agent".to_string(),
                runtime_id: "parity-runtime".to_string(),
                display_name: None,
                host: None,
                port: None,
                base_path: None,
                version: None,
                attributes: json!({}),
            },
            None,
            &runinator_models::auth::AuthContext::disabled_admin(),
        )
        .await
        .unwrap();
    let now = Utc::now();
    let directive = db
        .enqueue_agent_directive(
            replica.replica_id,
            AgentDirectiveKind::Diagnostics,
            now + Duration::minutes(5),
        )
        .await
        .unwrap();
    let claimed = db
        .claim_due_agent_directives(
            "publisher-a".to_string(),
            now,
            now - Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert!(
        db.claim_due_agent_directives(
            "publisher-b".to_string(),
            now,
            now - Duration::seconds(30),
            10,
        )
        .await
        .unwrap()
        .is_empty()
    );
    db.mark_agent_directive_published(directive.directive_id)
        .await
        .unwrap();
    let completed = db
        .complete_agent_directive(AgentDirectiveResult {
            directive_id: directive.directive_id,
            status: AgentDirectiveStatus::Completed,
            payload: json!({ "ok": true }),
            message: None,
        })
        .await
        .unwrap()
        .expect("directive remains readable");
    assert_eq!(completed.state, AgentDirectiveState::Completed);
    assert_eq!(
        db.list_agent_directives(replica.replica_id, 10)
            .await
            .unwrap()
            .len(),
        1
    );

    db.enqueue_agent_directive(
        replica.replica_id,
        AgentDirectiveKind::Restart,
        now - Duration::seconds(1),
    )
    .await
    .unwrap();
    assert_eq!(db.expire_agent_directives(now).await.unwrap(), 1);
}

async fn assert_agent_enrollment_lifecycle<T: DatabaseImpl>(db: &T) {
    let now = Utc::now();
    let token_id = "parity-enrollment".to_string();
    let token = AgentEnrollmentToken {
        token_id: token_id.clone(),
        org_id: Some(Uuid::new_v4()),
        labels: BTreeMap::from([("site".to_string(), "parity".to_string())]),
        service_url: "https://runinator.example".to_string(),
        spki_pin: Some("sha256/parity".to_string()),
        expires_at: now + Duration::minutes(5),
        consumed_at: None,
        issued_by: None,
        created_at: now,
    };
    db.create_agent_enrollment_token(AgentEnrollmentTokenRecord {
        token: token.clone(),
        sealed_secret: vec![1, 2, 3, 4],
    })
    .await
    .unwrap();

    let stored = db
        .fetch_agent_enrollment_token(token_id.clone())
        .await
        .unwrap()
        .expect("enrollment token is readable");
    assert_eq!(stored.token.labels, token.labels);
    assert_eq!(stored.sealed_secret, vec![1, 2, 3, 4]);
    assert_eq!(db.list_agent_enrollment_tokens().await.unwrap().len(), 1);

    let key_id = Uuid::new_v4();
    let principal_id = Uuid::new_v4();
    let key_record = ApiKeyRecord {
        key: ApiKey {
            id: Some(key_id),
            name: "parity agent".to_string(),
            user_id: Some(principal_id),
            is_service: true,
            key_prefix: "parityagent".to_string(),
            last_used_at: None,
            expires_at: None,
            disabled: false,
            created_at: now,
        },
        is_admin: false,
        principal_kind: PrincipalKind::Agent,
        org_id: token.org_id,
        key_hash: "parity-hash".to_string(),
    };
    assert!(
        db.consume_enrollment_token_and_create_api_key(token_id.clone(), key_record.clone(), now,)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        db.consume_enrollment_token_and_create_api_key(token_id.clone(), key_record, now)
            .await
            .unwrap()
            .is_none(),
        "an enrollment token can mint only one credential"
    );
    let key = db
        .fetch_api_key(key_id)
        .await
        .unwrap()
        .expect("agent key committed with token consumption");
    assert_eq!(key.principal_kind, PrincipalKind::Agent);
    assert_eq!(key.org_id, token.org_id);

    assert_eq!(db.purge_expired_enrollment_tokens(now).await.unwrap(), 1);
    assert!(
        db.fetch_agent_enrollment_token(token_id)
            .await
            .unwrap()
            .is_none()
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
// the compare-and-swap that keeps two cursors of one run from discarding each other's state. the
// interesting case is the losing writer: it must be told it lost rather than silently overwriting,
// and every dialect has to report the affected-row count for that to work.
async fn assert_run_state_cas<T: DatabaseImpl>(db: &T, run_id: Uuid) {
    let before = db.fetch_workflow_run(run_id).await.unwrap().expect("run");
    let version = before.state_version;

    assert!(
        db.update_workflow_run_state_cas(run_id, version, json!({ "watch_fired": true }))
            .await
            .unwrap(),
        "a write against the current version must land"
    );

    let after = db.fetch_workflow_run(run_id).await.unwrap().expect("run");
    assert_eq!(
        after.state_version,
        version + 1,
        "a landed write bumps the version"
    );
    assert_eq!(after.state.get("watch_fired"), Some(&Value::Bool(true)));

    assert!(
        !db.update_workflow_run_state_cas(run_id, version, json!({ "watch_fired": false }))
            .await
            .unwrap(),
        "a write against a stale version must be rejected"
    );
    let unchanged = db.fetch_workflow_run(run_id).await.unwrap().expect("run");
    assert_eq!(
        unchanged.state.get("watch_fired"),
        Some(&Value::Bool(true)),
        "the rejected write must not have applied"
    );

    // a plain status write also moves the version, so a reader that snapshotted before it cannot
    // then win a compare-and-swap against the blob it never saw.
    db.update_workflow_run_status(
        run_id,
        WorkflowStatus::Running,
        None,
        Some(json!({ "watch_fired": true, "run_metadata": { "n": 1 } })),
        None,
    )
    .await
    .unwrap();
    let bumped = db.fetch_workflow_run(run_id).await.unwrap().expect("run");
    assert_eq!(bumped.state_version, after.state_version + 1);
    assert!(
        !db.update_workflow_run_state_cas(run_id, after.state_version, json!({}))
            .await
            .unwrap(),
        "a status write that touched state must invalidate an earlier read"
    );
}

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
        .create_workflow_node_run(run.id, "task-1".into(), Value::Null, None, None)
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

// the cooldown gate admits exactly one caller per window, decided by an UPDATE's affected-row
// count and settled on first use by insert-or-ignore.
//
// both halves are dialect-shaped: `insert_ignore` renders three different ways, and the affected
// count for an UPDATE that matches a row but changes nothing is the classic mysql divergence
// (CLIENT_FOUND_ROWS). If mysql ever reported "matched" rather than "changed" here, a caller inside
// the window would be admitted, which is the gate silently not gating.
async fn assert_cooldown_claim<T: DatabaseImpl>(db: &T) {
    let now = Utc::now().timestamp();
    let name = "parity-gate".to_string();

    assert_eq!(
        db.claim_cooldown(name.clone(), 3600, now).await.unwrap(),
        None,
        "first use of a gate takes the window"
    );
    let held = db
        .claim_cooldown(name.clone(), 3600, now)
        .await
        .unwrap()
        .expect("a second caller inside the window is turned away");
    assert!(
        held > 0 && held <= 3600,
        "the loser is told how long is left, got {held}"
    );

    // re-claiming with the same timestamp must still refuse: an UPDATE that matched the row but
    // wrote an identical value reports zero changed rows on some engines and one on others, and
    // reading that as a win would admit everyone.
    assert!(
        db.claim_cooldown(name.clone(), 3600, now)
            .await
            .unwrap()
            .is_some(),
        "a repeat claim inside the window stays refused"
    );

    // once the window has elapsed the next caller takes it, and the one after is refused again.
    let later = now + 3601;
    assert_eq!(
        db.claim_cooldown(name.clone(), 3600, later).await.unwrap(),
        None,
        "an elapsed window is claimable"
    );
    assert!(
        db.claim_cooldown(name.clone(), 3600, later)
            .await
            .unwrap()
            .is_some(),
        "and claiming it re-closes the gate behind the winner"
    );

    // a zero-length window is always claimable and must not underflow the cutoff.
    let open = "parity-open".to_string();
    assert_eq!(db.claim_cooldown(open.clone(), 0, now).await.unwrap(), None);
    assert_eq!(db.claim_cooldown(open, 0, now).await.unwrap(), None);
}

// arming a wake supersedes the *same cursor's* earlier live generation, and nothing else.
//
// two threads of control can sit on one node — a fan-out whose branches converge, or a speculative
// fork walking beside the branch it came from. before the cursor scoped it, re-arming either one
// settled both, so a branch silently lost its pending wake and the run stalled with no error.
//
// this is dialect-sensitive twice over: `enqueue_ready_node` returns the inserted row through
// postgres `RETURNING` but through a second SELECT on mysql, and the supersede predicate is
// string-built per dialect because the three disagree on `? IS NULL`.
async fn assert_cursor_scoped_ready_nodes<T: DatabaseImpl>(db: &T, run_id: Uuid) {
    let node = "converge";
    let left = Uuid::now_v7();
    let right = Uuid::now_v7();
    let armed_at = Utc::now();

    let arm = async |cursor: Uuid| {
        db.enqueue_ready_node(
            NewOrchestrationEvent::new(run_id, Some(node.to_string()), "parity_arm", json!({}))
                .for_cursor(cursor),
            node.to_string(),
            armed_at,
        )
        .await
        .unwrap()
        .expect("a fresh event always inserts a row")
    };

    let left_first = arm(left).await;
    let right_first = arm(right).await;
    assert_eq!(
        left_first.cursor_id,
        Some(left),
        "the row must carry the cursor it was armed for, through both the RETURNING and the \
         read-back path"
    );

    let live = |rows: &[runinator_models::orchestration::ReadyNodeRecord]| {
        rows.iter()
            .filter(|row| row.node_id == node && row.completed_at.is_none())
            .count()
    };
    let pending = db.fetch_pending_ready_nodes(Utc::now(), 50).await.unwrap();
    assert_eq!(
        live(&pending),
        2,
        "two cursors on one node hold two live rows; a run-and-node-wide supersede would leave one"
    );

    // re-arm the left branch: it settles its own earlier generation and leaves the right alone.
    let left_second = arm(left).await;
    let pending = db.fetch_pending_ready_nodes(Utc::now(), 50).await.unwrap();
    let live_ids: Vec<Uuid> = pending
        .iter()
        .filter(|row| row.node_id == node && row.completed_at.is_none())
        .map(|row| row.id)
        .collect();
    assert!(
        !live_ids.contains(&left_first.id),
        "re-arming a cursor supersedes its own earlier generation"
    );
    assert!(
        live_ids.contains(&right_first.id),
        "re-arming one branch must not cancel a sibling's pending wake"
    );
    assert!(live_ids.contains(&left_second.id));

    // and both remain independently claimable and completable.
    for row in [left_second.id, right_first.id] {
        let claimed = db
            .claim_ready_node(
                row,
                "parity".into(),
                Utc::now(),
                Utc::now() + Duration::seconds(30),
            )
            .await
            .unwrap();
        assert!(claimed.is_some(), "each branch's row claims on its own");
        db.complete_ready_node(row, "parity".into()).await.unwrap();
    }
    let pending = db.fetch_pending_ready_nodes(Utc::now(), 50).await.unwrap();
    assert_eq!(live(&pending), 0, "both branches settle independently");
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
