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
use std::future::Future;

use chrono::{Duration, Utc};
use runinator_comm::{
    ActionCommand, AgentDirectiveKind, AgentDirectiveResult, AgentDirectiveState,
    AgentDirectiveStatus, EffectCommand, WorkflowResultEvent, WorkflowResultEventKind,
};
use runinator_models::{
    auth::{AgentEnrollmentToken, AgentEnrollmentTokenRecord, ApiKey, ApiKeyRecord, PrincipalKind},
    invocation::{CallableTarget, InvocationContinuation, NewInvocationCall},
    json,
    orchestration::NewOrchestrationEvent,
    revisions::{RevisionSource, WorkflowRevision},
    runs::RunStatus,
    settings::SettingKind,
    types::RuninatorType,
    value::Value,
    workflow_state::WorkflowExecutionState,
    workflow_vm::{
        WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowEffect,
        WorkflowEffectRequest, WorkflowEffectStatus, WorkflowInstruction, WorkflowModule,
    },
    workflows::{
        WorkflowAction, WorkflowDefinition, WorkflowGraph, WorkflowObject, WorkflowStatus,
        WorkflowTrigger, WorkflowTriggerKind,
    },
};
use runinator_store::workflow_mutex::WorkflowMutexClaim;
use uuid::Uuid;

// `DatabaseImpl` composes every role trait, so bounding on it brings all of their methods into
// scope without importing the roles one by one.
use crate::backend::{SqlBackend, SqlStore};
use crate::interfaces::DatabaseImpl;
use runinator_models::errors::SendableError;
use runinator_store::roles::WorkflowVmStore;
use sqlx::{Database, Encode, Executor, IntoArguments, Type};

pub(crate) trait ExecutionStateParityDb: DatabaseImpl + WorkflowVmStore {
    fn stage_legacy_execution_state(
        &self,
        workflow_run_id: Uuid,
        state: WorkflowExecutionState,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}

impl<B> ExecutionStateParityDb for SqlStore<B>
where
    B: SqlBackend,
    SqlStore<B>: DatabaseImpl + WorkflowVmStore,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    async fn stage_legacy_execution_state(
        &self,
        workflow_run_id: Uuid,
        state: WorkflowExecutionState,
    ) -> Result<(), SendableError> {
        let mut tx = self.pool().begin().await?;
        for table in [
            "workflow_run_pending_interrupts",
            "workflow_run_event_sources",
            "workflow_run_cursors",
            "workflow_run_frames",
            "workflow_run_execution_states",
        ] {
            sqlx::query(&self.render(&format!("DELETE FROM {table} WHERE workflow_run_id = ?")))
                .bind(workflow_run_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(&self.render("UPDATE workflow_runs SET state = ? WHERE id = ?"))
            .bind(state.to_state().to_string())
            .bind(workflow_run_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

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
            function_binding: None,
        },
        attempt: 1,
        parameters: runinator_models::json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        invocation_call_id: None,
        task_run_id: None,
        idempotency_key: None,
    }
}

/// run the full cross-dialect lifecycle against an already-migrated, empty store.
///
/// the store must be exclusive to this call: several assertions count rows or depend on a claim
/// finding nothing else outstanding.
pub(crate) async fn assert_dialect_parity<T: ExecutionStateParityDb>(db: &T) {
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
    assert_normalized_execution_state_lifecycle(db, &after).await;
    assert_cursor_scoped_ready_nodes(db, run_id).await;
    assert_cooldown_claim(db).await;
    assert_workflow_mutex_claim(db, &after).await;
    assert_agent_enrollment_lifecycle(db).await;
    assert_agent_directive_lifecycle(db).await;
    assert_function_lifecycle(db, id).await;
    assert_console_lifecycle(db).await;
    assert_invocation_lifecycle(db, run_id, node_id).await;
    assert_workflow_vm_readback(db, &after).await;
    assert_unreferenced_artifacts(db).await;

    // the legacy run mapper reads a column named `trigger`, which is reserved in mysql and has to
    // be quoted per dialect; an unquoted build fails here rather than in production.
    assert!(
        db.fetch_runs_by_status(RunStatus::Running)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn assert_workflow_vm_readback<T: DatabaseImpl + WorkflowVmStore>(
    db: &T,
    workflow: &WorkflowDefinition,
) {
    let snapshot = db
        .fetch_workflow(workflow.id.expect("workflow id"))
        .await
        .unwrap()
        .expect("workflow snapshot");
    let run = db
        .create_workflow_run(
            snapshot.id.expect("workflow id"),
            snapshot,
            Value::Null,
            Value::Null,
            None,
            Default::default(),
        )
        .await
        .unwrap();
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::TimerDelay { seconds: 1 },
        },
        WorkflowInstruction::Return,
    ]);
    let root = WorkflowContinuation::start(run.id, module.version);
    db.create_workflow_vm(module.clone(), root.clone())
        .await
        .unwrap();
    assert_eq!(
        db.fetch_workflow_module(run.id).await.unwrap(),
        Some(module.clone())
    );
    assert_eq!(
        db.fetch_workflow_continuations(run.id).await.unwrap(),
        vec![root.clone()]
    );

    let claimed = db
        .claim_runnable_workflow_continuations(
            "parity-scheduler".into(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            1,
        )
        .await
        .unwrap();
    let runinator_runtime::WorkflowVmStep::Yield {
        continuation,
        effect_id,
        sequence,
        request,
    } = runinator_runtime::step_workflow_vm(&module, claimed.into_iter().next().unwrap())
    else {
        panic!("expected an effect yield");
    };
    let now = Utc::now().timestamp();
    let effect = WorkflowEffect {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        id: effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        sequence,
        attempt: 0,
        request: request.clone(),
        status: WorkflowEffectStatus::Requested,
        result: None,
        message: None,
        created_at: now,
        updated_at: now,
        finished_at: None,
    };
    let command = EffectCommand {
        version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
        command_id: Uuid::now_v7(),
        effect_id,
        workflow_run_id: run.id,
        continuation_id: continuation.id,
        attempt: 0,
        request,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: effect.idempotency_key(),
    };
    db.suspend_on_effect(continuation, effect.clone(), command)
        .await
        .unwrap();
    assert_eq!(
        db.fetch_workflow_effect(effect_id).await.unwrap(),
        Some(effect.clone())
    );
    assert_eq!(
        db.fetch_workflow_effects(run.id).await.unwrap(),
        vec![effect]
    );
    let journal = db.fetch_workflow_journal(run.id).await.unwrap();
    assert_eq!(journal.len(), 2);
    assert_eq!(journal[0].sequence, 0);
    assert_eq!(journal[1].sequence, 1);
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
            &runinator_models::auth::AuthContext::disabled_platform_admin(),
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
            principal_kind: PrincipalKind::Service,
            principal_id,
            system_role: Some(runinator_models::rbac::SystemRole::Agent),
            org_id: token.org_id,
            action_ceiling: Vec::new(),
            key_prefix: "parityagent".to_string(),
            last_used_at: None,
            expires_at: None,
            disabled: false,
            created_at: now,
        },
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
    assert_eq!(key.key.principal_kind, PrincipalKind::Service);
    assert_eq!(key.key.org_id, token.org_id);

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

    assert_due_set_skips_disabled_workflow(db, workflow_id, trigger_id).await;

    let mut retrig = saved.clone();
    retrig.enabled = false;
    let retrigged = db.upsert_workflow_trigger(&retrig).await.unwrap();
    assert_eq!(retrigged.id, Some(trigger_id));
    assert!(!retrigged.enabled);
}

/// an enabled trigger on a *disabled* workflow is not due and cannot be claimed. the predicate is a
/// correlated `EXISTS` carrying a per-dialect boolean literal, so it is exactly the kind of thing
/// that works on one engine and silently matches nothing on another.
async fn assert_due_set_skips_disabled_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: Uuid,
    trigger_id: Uuid,
) {
    let now = Utc::now();
    let contains_trigger = |triggers: Vec<WorkflowTrigger>| {
        triggers
            .iter()
            .any(|trigger| trigger.id == Some(trigger_id))
    };

    assert!(
        contains_trigger(db.fetch_due_workflow_triggers(now).await.unwrap()),
        "a trigger on an enabled workflow is due"
    );

    let mut workflow = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    workflow.enabled = false;
    db.upsert_workflow(&workflow).await.unwrap();

    assert!(
        !contains_trigger(db.fetch_due_workflow_triggers(now).await.unwrap()),
        "a trigger on a disabled workflow is not due"
    );
    let claimed = db
        .claim_due_workflow_trigger_firings("parity".to_string(), now, 10)
        .await
        .unwrap();
    assert!(
        claimed.runs.is_empty(),
        "a disabled workflow's trigger is not claimable"
    );

    workflow.enabled = true;
    db.upsert_workflow(&workflow).await.unwrap();
    assert!(
        contains_trigger(db.fetch_due_workflow_triggers(now).await.unwrap()),
        "re-enabling the workflow puts its trigger back in the due set"
    );
}

// the scheduler claim is a multi-row conditional UPDATE plus a read of the rows it took. dialects
// without RETURNING emulate it with a marker column and a follow-up SELECT, so a broken lease
// shows up here as a second scheduler claiming work the first already holds.
// the compare-and-swap that keeps two cursors of one run from discarding each other's state. the
// interesting case is the losing writer: it must be told it lost rather than silently overwriting,
// and every dialect has to report the affected-row count for that to work.
fn complex_execution_state(revision: i64) -> WorkflowExecutionState {
    let primary = Uuid::from_u128(0x100);
    let branch = Uuid::from_u128(0x200);
    let handler = Uuid::from_u128(0x300);
    let loop_run = Uuid::from_u128(0x400);
    let requested = Uuid::from_u128(0x500 + revision as u128);
    WorkflowExecutionState::from_state(&json!({
        "cursors": [
            {
                "id": primary,
                "node_id": format!("approval-{revision}"),
                "loops": [{
                    "node_id": "outer-loop",
                    "index": revision,
                    "items": [1, { "nested": true }],
                    "results": [{ "lap": 0 }],
                    "last_node_run_id": loop_run
                }],
                "try": {
                    "node_id": "try-region",
                    "phase": "catch",
                    "pending_status": "failed",
                    "pending_output": { "error": "retryable" }
                },
                "debug": {
                    "paused": true,
                    "step_requested": revision > 1,
                    "one_shot_breakpoint": "resume-here",
                    "current_node_id": format!("approval-{revision}"),
                    "current_node_kind": "approval",
                    "input_json": { "revision": revision },
                    "context_json": { "tenant": "parity" },
                    "last_output_json": { "prior": revision - 1 }
                },
                "last_output": { "cursor": "primary", "revision": revision },
                "suspended_by": handler,
                "handled": [format!("timeout:{loop_run}:1")],
                "suspended_seconds": 17 + revision
            },
            {
                "id": branch,
                "node_id": "parallel-branch",
                "forked_by": "parallel-root",
                "loops": [{
                    "node_id": "branch-loop",
                    "index": 2,
                    "items": ["a", "b", "c"],
                    "results": ["A", "B"]
                }],
                "try": { "node_id": "branch-try", "phase": "finally" },
                "debug": {
                    "current_node_id": "parallel-branch",
                    "current_node_kind": "task",
                    "context_json": { "branch": true }
                },
                "last_output": ["branch", revision]
            },
            {
                "id": handler,
                "node_id": "interrupt-handler",
                "interrupt": {
                    "interrupted_cursor": primary,
                    "source": "timeout",
                    "payload": { "deadline": "expired", "revision": revision },
                    "resume": {
                        "node_id": format!("approval-{revision}"),
                        "loops": [{
                            "node_id": "outer-loop",
                            "index": revision,
                            "items": [1, { "nested": true }],
                            "results": [{ "lap": 0 }],
                            "last_node_run_id": loop_run
                        }],
                        "try_frame": {
                            "node_id": "try-region",
                            "phase": "catch",
                            "pending_status": "failed"
                        }
                    },
                    "raised_at": "2026-08-14T12:34:56Z"
                },
                "last_output": { "handler": "running" }
            }
        ],
        "control": { "pause_requested": true, "reason": "parity" },
        "debug": {
            "enabled": true,
            "mode": "breakpoints",
            "breakpoints": ["approval-1", "parallel-branch"],
            "paused": true,
            "current_node_id": format!("approval-{revision}"),
            "current_node_kind": "approval",
            "context_json": { "scope": "run" }
        },
        "event_sources": {
            "webhook": { "pending_event": { "kind": "push", "revision": revision } },
            "empty-slot": {}
        },
        "run_metadata": { "request_id": format!("parity-{revision}"), "attempt": revision },
        "watch_fired": revision > 1,
        "pending_interrupts": [
            {
                "id": requested,
                "source": "external",
                "payload": { "command": "refresh", "revision": revision },
                "cursor_id": branch,
                "requested_at": "2026-08-14T12:35:00Z"
            },
            {
                "id": Uuid::from_u128(0x600 + revision as u128),
                "source": "orphan_signal",
                "payload": { "signal": "unmatched" },
                "requested_at": "2026-08-14T12:35:01Z"
            }
        ],
        "custom_state": { "kept": true, "revision": revision }
    }))
}

async fn assert_normalized_execution_state_lifecycle<T: ExecutionStateParityDb>(
    db: &T,
    workflow: &WorkflowDefinition,
) {
    let initial = complex_execution_state(1);
    let created = db
        .create_workflow_run(
            workflow.id.expect("workflow id"),
            workflow.clone(),
            json!({ "source": "normalized-parity" }),
            initial.to_state(),
            Some("normalized execution state parity".into()),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(created.execution_state.to_state(), initial.to_state());
    assert_eq!(
        db.fetch_workflow_run(created.id)
            .await
            .unwrap()
            .expect("created run")
            .execution_state
            .to_state(),
        initial.to_state(),
        "create and fetch must preserve every normalized projection"
    );

    let updated = complex_execution_state(2);
    assert!(
        db.update_workflow_run_execution_state_cas(
            created.id,
            created.state_version,
            updated.clone(),
        )
        .await
        .unwrap()
    );
    let fetched = db
        .fetch_workflow_run(created.id)
        .await
        .unwrap()
        .expect("updated run");
    assert_eq!(fetched.state_version, created.state_version + 1);
    assert_eq!(fetched.execution_state.to_state(), updated.to_state());
    assert_eq!(fetched.state, json!({}), "the legacy blob stays cleared");

    let legacy = complex_execution_state(3);
    let legacy_run = db
        .create_workflow_run(
            workflow.id.expect("workflow id"),
            workflow.clone(),
            json!({ "source": "legacy-parity" }),
            json!({}),
            Some("legacy execution state parity".into()),
            Default::default(),
        )
        .await
        .unwrap();
    db.stage_legacy_execution_state(legacy_run.id, legacy.clone())
        .await
        .unwrap();
    let (left, right) = tokio::join!(
        db.migrate_workflow_execution_states(),
        db.migrate_workflow_execution_states(),
    );
    left.unwrap();
    right.unwrap();
    let backfilled = db
        .fetch_workflow_run(legacy_run.id)
        .await
        .unwrap()
        .expect("backfilled run");
    assert_eq!(backfilled.execution_state.to_state(), legacy.to_state());
    assert_eq!(
        backfilled.state,
        json!({}),
        "backfill clears the legacy blob"
    );
}

async fn assert_run_state_cas<T: DatabaseImpl>(db: &T, run_id: Uuid) {
    let before = db.fetch_workflow_run(run_id).await.unwrap().expect("run");
    let version = before.state_version;

    assert!(
        db.update_workflow_run_execution_state_cas(
            run_id,
            version,
            WorkflowExecutionState::from_state(&json!({ "watch_fired": true })),
        )
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
    assert!(after.execution_state.watch_fired);

    assert!(
        !db.update_workflow_run_execution_state_cas(
            run_id,
            version,
            WorkflowExecutionState::from_state(&json!({ "watch_fired": false })),
        )
        .await
        .unwrap(),
        "a write against a stale version must be rejected"
    );
    let unchanged = db.fetch_workflow_run(run_id).await.unwrap().expect("run");
    assert!(
        unchanged.execution_state.watch_fired,
        "the rejected write must not have applied"
    );

    // a plain status write also moves the version, so a reader that snapshotted before it cannot
    // then win a compare-and-swap against the blob it never saw.
    db.update_workflow_run_status(
        run_id,
        WorkflowStatus::Running,
        None,
        Some(WorkflowExecutionState::from_state(&json!({
            "watch_fired": true,
            "run_metadata": { "n": 1 }
        }))),
        None,
    )
    .await
    .unwrap();
    let bumped = db.fetch_workflow_run(run_id).await.unwrap().expect("run");
    assert_eq!(bumped.state_version, after.state_version + 1);
    assert!(
        !db.update_workflow_run_execution_state_cas(
            run_id,
            after.state_version,
            WorkflowExecutionState::default(),
        )
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
        invocation_call_id: None,
        task_run_id: None,
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

/// the resumable-invocation lifecycle: create, suspend on a call, settle the call, retry, and
/// settle the invocation.
///
/// the two assertions worth having here are the ones a single-engine test would miss: that
/// `suspend_invocation` is idempotent by `(invocation, sequence)` — which depends on a unique index
/// each dialect declares in its own file — and that `settle_invocation_call` refuses a stale
/// attempt, which depends on the `status IN (...)` guard rendering correctly per dialect.
async fn assert_invocation_lifecycle<T: DatabaseImpl>(db: &T, run_id: Uuid, node_id: Uuid) {
    let continuation = InvocationContinuation::start();
    let invocation = db
        .create_invocation(
            run_id,
            node_id,
            Some(Uuid::now_v7()),
            "invoke-1",
            runinator_models::invocation::INVOCATION_IR_VERSION,
            &continuation,
        )
        .await
        .unwrap();
    assert_eq!(invocation.status, WorkflowStatus::Running);
    assert_eq!(invocation.continuation, continuation);

    let call_id = Uuid::now_v7();
    let new_call = || NewInvocationCall {
        id: call_id,
        invocation_id: invocation.id,
        workflow_run_id: run_id,
        sequence: 0,
        target: CallableTarget::Provider {
            provider: "test".into(),
            function: "execute".into(),
        },
        arguments: vec![runinator_models::value::Value::from(1_i64)],
        policy: Default::default(),
        idempotency_key: Some("key-1".into()),
        deadline_at: Some(Utc::now().timestamp() + 60),
    };

    let call = db
        .suspend_invocation(&continuation, new_call(), sample_action(run_id, node_id))
        .await
        .unwrap();
    // a re-drive reaches the same sequence and must not create a second call.
    let again = db
        .suspend_invocation(&continuation, new_call(), sample_action(run_id, node_id))
        .await
        .unwrap();
    assert_eq!(
        call.id, again.id,
        "a duplicate sequence returns the same call"
    );
    assert_eq!(
        db.fetch_invocation_calls(invocation.id)
            .await
            .unwrap()
            .len(),
        1
    );

    let pending = db
        .fetch_pending_invocation_call(invocation.id)
        .await
        .unwrap()
        .expect("the invocation is parked on its call");
    assert_eq!(pending.id, call.id);
    assert_eq!(pending.attempt, 0);
    assert_eq!(pending.idempotency_key.as_deref(), Some("key-1"));

    // a real replica id: the lease columns carry a foreign key, so an invented uuid would fail here
    // on every engine rather than exercise the claim.
    let executor = db
        .register_replica(
            runinator_models::replicas::ReplicaRegistrationRequest {
                replica_type: runinator_models::replicas::ReplicaKind::Worker,
                instance_id: "parity-invocation-executor".to_string(),
                runtime_id: "parity-invocation-runtime".to_string(),
                display_name: None,
                host: None,
                port: None,
                base_path: None,
                version: None,
                attributes: json!({}),
            },
            None,
            &runinator_models::auth::AuthContext::disabled_platform_admin(),
        )
        .await
        .unwrap();
    db.set_invocation_call_executor(call.id, Some(executor.replica_id))
        .await
        .unwrap();
    let claimed = db.fetch_invocation_call(call.id).await.unwrap().unwrap();
    assert_eq!(
        claimed.current_executor_replica_id,
        Some(executor.replica_id)
    );
    db.set_invocation_call_executor(call.id, None)
        .await
        .unwrap();
    assert!(
        db.fetch_invocation_call(call.id)
            .await
            .unwrap()
            .unwrap()
            .current_executor_replica_id
            .is_none()
    );

    // a result naming the wrong attempt is discarded rather than applied.
    assert!(
        !db.settle_invocation_call(call.id, 7, WorkflowStatus::Succeeded, None, None)
            .await
            .unwrap()
    );
    assert!(
        db.settle_invocation_call(
            call.id,
            0,
            WorkflowStatus::Failed,
            None,
            Some("boom".into()),
        )
        .await
        .unwrap()
    );
    // and a duplicate of one already applied is discarded too.
    assert!(
        !db.settle_invocation_call(call.id, 0, WorkflowStatus::Succeeded, None, None)
            .await
            .unwrap()
    );

    let retried = db
        .retry_invocation_call(call.id, None, sample_action(run_id, node_id))
        .await
        .unwrap();
    assert_eq!(retried.attempt, 1);
    assert_eq!(retried.status, WorkflowStatus::Running);
    assert!(retried.finished_at.is_none());
    assert_eq!(
        retried.dispatch_key(),
        format!("workflow-invocation-call:{}:1", call.id)
    );

    assert_eq!(
        db.cancel_invocation_calls_for_run(run_id, "run canceled")
            .await
            .unwrap(),
        1
    );
    assert!(
        db.fetch_pending_invocation_call(invocation.id)
            .await
            .unwrap()
            .is_none()
    );

    // a chunk from a call is attributed to both the node run and the call. this is written by
    // `apply_workflow_result_event`, whose insert lists the column explicitly — a dialect that
    // rejected it would fail here rather than silently dropping call attribution.
    let chunk_event = WorkflowResultEvent {
        command_id: Uuid::now_v7(),
        event_id: Uuid::now_v7(),
        workflow_run_id: run_id,
        workflow_node_run_id: node_id,
        node_id: "invoke-1".into(),
        attempt: 0,
        kind: WorkflowResultEventKind::Chunk {
            chunk: runinator_models::runs::NewRunChunk {
                stream: "stdout".into(),
                content: "hello".into(),
            },
        },
        timestamp: Utc::now(),
        trace_id: Uuid::nil(),
        notification_delivery_id: None,
        invocation_call_id: Some(call.id),
        task_run_id: None,
    };
    assert!(db.apply_workflow_result_event(&chunk_event).await.unwrap());

    db.settle_invocation(
        invocation.id,
        WorkflowStatus::Succeeded,
        Some(runinator_models::value::Value::from(42_i64)),
        None,
    )
    .await
    .unwrap();
    let settled = db.fetch_invocation(invocation.id).await.unwrap().unwrap();
    assert_eq!(settled.status, WorkflowStatus::Succeeded);
    assert!(settled.finished_at.is_some());
    assert_eq!(
        db.fetch_invocation_for_node_run(node_id)
            .await
            .unwrap()
            .map(|item| item.id),
        Some(invocation.id)
    );
    assert_eq!(db.fetch_invocations_for_run(run_id).await.unwrap().len(), 1);
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

    // a mirror row (a packaged function's `functions.<pkg>` provider metadata) has to be removable
    // when the thing it mirrors is deleted, or it outlives its package advertising exports nothing
    // can run. `affected()` is what reports whether anything was deleted, and mysql and postgres
    // disagree enough about row counts that this is worth asserting on every engine.
    assert!(
        db.delete_catalog_item("cat://x".into()).await.unwrap(),
        "deleting an existing catalog item must report that it happened"
    );
    assert!(
        db.fetch_catalog_item("cat://x".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        !db.delete_catalog_item("cat://x".into()).await.unwrap(),
        "deleting a missing catalog item must report that nothing happened"
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

/// publish, alias, catalog, and the two refusals that keep a pinned version runnable.
///
/// exercised on every engine because the identity key, the version-number assignment, and the
/// alias upsert all lean on conflict handling that each dialect spells differently.
// the console's scope lives in the database rather than a replica's memory, so every engine has to
// agree about replacing a binding in place and about deleting a session's children.
// what a retention sweep reads. the query is a left join rather than `NOT IN` because the engines
// disagree about how `NOT IN` treats a null in the subquery, so this has to be asserted on each.
async fn assert_unreferenced_artifacts<T: DatabaseImpl>(db: &T) {
    use runinator_models::functions::FunctionArtifact;

    let orphan = FunctionArtifact {
        digest: format!("sha256:{}", "c".repeat(64)),
        size_bytes: 10,
        uri: "blob://runinator-function-artifacts/sha256/cc/cc/c.zip".into(),
        media_type: "application/zip".into(),
        created_at: Utc::now(),
    };
    db.upsert_function_artifact(&orphan).await.unwrap();

    let unreferenced = db.fetch_unreferenced_function_artifacts().await.unwrap();
    assert!(
        unreferenced.iter().any(|item| item.digest == orphan.digest),
        "an artifact no version references must be sweepable"
    );
    // and an artifact a version pins must never appear, or a sweep would delete live code.
    assert!(
        unreferenced
            .iter()
            .all(|item| item.digest != format!("sha256:{}", "a".repeat(64))),
        "a referenced artifact must not be listed as unreferenced"
    );
}

async fn assert_console_lifecycle<T: DatabaseImpl>(db: &T) {
    use runinator_models::console::{ConsoleCellKind, ConsoleCellStatus, NewConsoleCell};

    let session = db
        .create_console_session(None, "scratch", None)
        .await
        .unwrap();
    assert_eq!(session.name, "scratch");

    // positions are assigned by append when the caller does not name one.
    let first = db
        .upsert_console_cell(
            session.id,
            None,
            &NewConsoleCell {
                source: "1 + 2".into(),
                label: Some("sum".into()),
                position: None,
            },
        )
        .await
        .unwrap();
    let second = db
        .upsert_console_cell(
            session.id,
            None,
            &NewConsoleCell {
                source: "cells.sum * 2".into(),
                label: None,
                position: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(first.position, 0);
    assert_eq!(second.position, 1);
    assert_eq!(first.status, ConsoleCellStatus::Idle);

    let outcome = db
        .record_console_cell_outcome(
            first.id,
            Some(ConsoleCellKind::Expression),
            ConsoleCellStatus::Succeeded,
            Some(&runinator_models::json!(3)),
            None,
            None,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outcome.status, ConsoleCellStatus::Succeeded);
    assert_eq!(outcome.result, Some(runinator_models::json!(3)));
    assert_eq!(outcome.kind, Some(ConsoleCellKind::Expression));

    // re-running a cell must replace its binding rather than add a second row for the same name.
    db.upsert_console_binding(
        session.id,
        "sum",
        Some(first.id),
        &runinator_models::json!(3),
    )
    .await
    .unwrap();
    db.upsert_console_binding(
        session.id,
        "sum",
        Some(first.id),
        &runinator_models::json!(4),
    )
    .await
    .unwrap();
    let bindings = db.fetch_console_bindings(session.id).await.unwrap();
    assert_eq!(bindings.len(), 1, "a name must bind once per session");
    assert_eq!(bindings[0].value, runinator_models::json!(4));

    // editing a cell clears its outcome: a result beside changed source is a stale answer shown as
    // a current one.
    let edited = db
        .upsert_console_cell(
            session.id,
            Some(first.id),
            &NewConsoleCell {
                source: "1 + 40".into(),
                label: Some("sum".into()),
                position: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.status, ConsoleCellStatus::Idle);
    assert!(edited.result.is_none());
    assert!(edited.kind.is_none());

    // a scratch run is found from its run id, which is how a settled run is attributed back.
    let run_id = Uuid::new_v4();
    db.record_console_cell_outcome(
        second.id,
        Some(ConsoleCellKind::Workflow),
        ConsoleCellStatus::Running,
        None,
        None,
        Some(run_id),
    )
    .await
    .unwrap();
    assert_eq!(
        db.fetch_console_cell_for_run(run_id)
            .await
            .unwrap()
            .map(|cell| cell.id),
        Some(second.id)
    );

    // deleting a cell takes its binding with it.
    assert!(db.delete_console_cell(first.id).await.unwrap());
    assert!(
        db.fetch_console_bindings(session.id)
            .await
            .unwrap()
            .is_empty()
    );

    // and deleting the session takes the rest, explicitly rather than by cascade.
    assert!(db.delete_console_session(session.id).await.unwrap());
    assert!(db.fetch_console_cells(session.id).await.unwrap().is_empty());
    assert!(
        db.fetch_console_session(session.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!db.delete_console_session(session.id).await.unwrap());
}

async fn assert_function_lifecycle<T: DatabaseImpl>(db: &T, workflow_id: Uuid) {
    use runinator_models::functions::{
        FunctionArtifact, NewFunctionExport, NewFunctionPackage, NewFunctionVersion,
    };
    use runinator_models::providers::{ParameterMetadata, ResultMetadata};
    use runinator_models::types::RuninatorType;

    let digest = format!("sha256:{}", "1".repeat(64));
    let artifact = FunctionArtifact {
        digest: digest.clone(),
        size_bytes: 1234,
        uri: format!("blob://runinator-function-artifacts/sha256/{digest}.zip"),
        media_type: "application/zip".into(),
        created_at: Utc::now(),
    };
    let stored = db.upsert_function_artifact(&artifact).await.unwrap();
    assert_eq!(stored.digest, digest);
    // content-addressed: storing the same bytes twice is a no-op rather than a duplicate or an error.
    db.upsert_function_artifact(&artifact).await.unwrap();

    let request = NewFunctionVersion {
        package: NewFunctionPackage {
            name: "image-tools".into(),
            namespace: None,
            description: Some("image helpers".into()),
            org_id: None,
        },
        artifact_digest: digest.clone(),
        manifest: runinator_models::json!({ "package": { "name": "image-tools" } }),
        runtime: runinator_models::functions::FunctionRuntimeSpec::new("python3.13"),
        exports: vec![NewFunctionExport {
            name: "resize".into(),
            handler: "src.images.resize".into(),
            description: Some("resize an image".into()),
            input: vec![ParameterMetadata::required("source", RuninatorType::String)],
            output: vec![ResultMetadata::new("uri", RuninatorType::String)],
            limits: Default::default(),
        }],
        alias: Some("production".into()),
    };

    let first = db.publish_function_version(&request).await.unwrap();
    assert_eq!(first.version, 1);
    assert_eq!(first.artifact_digest, digest);
    assert_eq!(first.runtime.runtime, "python3.13");

    // republishing the same package must advance the version rather than collide on identity.
    let second = db.publish_function_version(&request).await.unwrap();
    assert_eq!(second.version, 2);
    assert_eq!(second.package_id, first.package_id);

    let package = db
        .fetch_function_package(None, None, "image-tools")
        .await
        .unwrap()
        .expect("package by identity");
    assert_eq!(package.id, first.package_id);
    assert_eq!(package.latest_version, Some(2));

    let versions = db.fetch_function_versions(package.id).await.unwrap();
    assert_eq!(versions.len(), 2);
    // newest first, so a listing shows the current release at the top.
    assert_eq!(versions[0].version, 2);

    let exports = db.fetch_function_exports(first.id).await.unwrap();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0].name, "resize");
    assert_eq!(exports[0].input.len(), 1);
    assert!(
        !exports[0].limits.network,
        "network stays opt-in through a round trip"
    );

    // publishing moved the alias to the newest version.
    let alias = db
        .fetch_function_alias(package.id, "production")
        .await
        .unwrap()
        .expect("alias");
    assert_eq!(alias.version, 2);

    // and it can be pointed back, which is what a rollback is.
    let rolled_back = db
        .set_function_alias(package.id, "production", first.id)
        .await
        .unwrap();
    assert_eq!(rolled_back.version, 1);
    assert_eq!(
        db.fetch_function_aliases(package.id).await.unwrap().len(),
        1
    );

    let catalog = db.fetch_function_catalog().await.unwrap();
    assert_eq!(catalog.len(), 2, "one entry per export per version");
    let pinned = catalog
        .iter()
        .find(|entry| entry.version == 1)
        .expect("version 1 stays in the catalog so a pinned workflow still type-checks");
    assert_eq!(pinned.provider_name(), "functions.image-tools");
    assert_eq!(pinned.aliases, vec!["production".to_string()]);
    assert_eq!(pinned.binding().artifact_digest, digest);

    // an artifact a published version pins cannot be deleted out from under it.
    assert!(db.delete_function_artifact(&digest).await.is_err());

    let adapter = db
        .upsert_function_adapter_workflow(pinned.export_id, workflow_id)
        .await
        .unwrap();
    assert_eq!(adapter.workflow_id, workflow_id);
    assert_eq!(
        db.fetch_function_adapter_workflow(pinned.export_id)
            .await
            .unwrap()
            .map(|record| record.workflow_id),
        Some(workflow_id)
    );

    assert!(
        db.delete_function_alias(package.id, "production")
            .await
            .unwrap()
    );
    assert!(db.delete_function_package(package.id).await.unwrap());
    // archival removes authoring visibility without invalidating immutable bindings.
    assert!(db.fetch_function_catalog().await.unwrap().is_empty());
    let archived = db
        .fetch_function_package_by_id(package.id)
        .await
        .unwrap()
        .expect("archived package remains");
    assert!(archived.archived_at.is_some());
    assert!(db.delete_function_artifact(&digest).await.is_err());

    assert!(db.restore_function_package(package.id).await.unwrap());
    assert_eq!(db.fetch_function_catalog().await.unwrap().len(), 2);
}
