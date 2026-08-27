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
//! engine when their URL is set. Running it on SQLite keeps it from rotting in a workspace
//! where nobody has docker up.

use chrono::{Duration, Utc};
use runinator_comm::{
    AgentDirectiveKind, AgentDirectiveResult, AgentDirectiveState, AgentDirectiveStatus,
    EffectCommand,
};
use runinator_models::{
    auth::{AgentEnrollmentToken, AgentEnrollmentTokenRecord, ApiKey, ApiKeyRecord, PrincipalKind},
    json,
    orchestration::{
        ControlEffect, DeliverySemantics, ExternalOperation, ExternalOperationStatus,
        IngressAdmission, IngressAdmissionClaim, IngressAdmissionStatus, IngressEvent,
        IngressEventDisposition, IngressTarget, IngressTargetKind, IntentPolicy,
        NewOrchestrationBinding, OrchestrationEventReduction, OrchestrationPendingIntent,
        OrchestrationPolicy, OrchestrationStatus,
    },
    pipelines::{Pipeline, PipelineGraph, PipelineMember, PipelineMemberFailureMode},
    revisions::{RevisionSource, WorkflowRevision},
    settings::SettingKind,
    types::RuninatorType,
    value::Value,
    workflow_state::WorkflowExecutionState,
    workflow_vm::{
        WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowContinuation, WorkflowEffect,
        WorkflowEffectRequest, WorkflowEffectStatus, WorkflowInstruction, WorkflowModule,
    },
    workflows::{
        WorkflowDefinition, WorkflowGraph, WorkflowStatus, WorkflowTrigger, WorkflowTriggerKind,
    },
};
use std::collections::BTreeMap;
use uuid::Uuid;

// `DatabaseImpl` composes every role trait, so bounding on it brings all of their methods into
// scope without importing the roles one by one.
use runinator_store::DatabaseImpl;
use runinator_store::roles::{
    ExternalOperationUpdate, NewAdapterDefinition, NewOrchestrationCommand, NewOrchestrationEpoch,
    OrchestrationBindingUpdate, WorkflowVmStore,
};

fn sample_workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        key: None,
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

/// run the full cross-dialect lifecycle against an already-migrated, empty store.
///
/// the store must be exclusive to this call: several assertions count rows or depend on a claim
/// finding nothing else outstanding.
pub(crate) async fn assert_dialect_parity<T: DatabaseImpl + WorkflowVmStore>(db: &T) {
    assert_workflow_upsert(db).await;
    let after = db.fetch_workflows().await.unwrap().remove(0);
    let id = after.id.expect("the upserted workflow has an id");

    assert_revision_history(db, &after).await;
    assert_trigger_upsert(db, id).await;
    assert_idempotency_keys(db).await;
    assert_ingress_admission_claim(db).await;
    assert_correlated_orchestration_lifecycle(db, id).await;
    assert_notifications(db).await;
    assert_settings(db).await;
    assert_catalog_upsert(db).await;
    assert_automation_records(db, Uuid::now_v7()).await;
    assert_normalized_execution_state_lifecycle(db, &after).await;
    assert_cooldown_claim(db).await;
    assert_agent_enrollment_lifecycle(db).await;
    assert_agent_directive_lifecycle(db).await;
    assert_function_lifecycle(db, id).await;
    assert_console_lifecycle(db).await;
    assert_workflow_vm_readback(db, &after).await;
    assert_workflow_vm_mutex_lifecycle(db, &after).await;
    assert_workflow_effect_retry_lifecycle(db, &after).await;
    assert_unreferenced_artifacts(db).await;
}

/// Exercise the orchestration-specific migration and every atomic primitive that differs by SQL
/// dialect: idempotent inserts, leased claims, binding CAS, pending-intent consumption, and both
/// command/provider outboxes. Live Postgres/MySQL suites call this exact function too.
async fn assert_correlated_orchestration_lifecycle<T: DatabaseImpl + WorkflowVmStore>(
    db: &T,
    workflow_id: Uuid,
) {
    let now = Utc::now();
    let suffix = Uuid::now_v7();
    let pipeline = db
        .upsert_pipeline(&Pipeline {
            id: None,
            name: format!("orchestration parity {suffix}"),
            key: Some(format!("orchestration_parity_{suffix}")),
            namespace: Some("dialect_parity".into()),
            description: None,
            org_id: None,
            graph: PipelineGraph {
                version: runinator_models::pipelines::PIPELINE_GRAPH_VERSION,
                members: vec![PipelineMember {
                    key: "member".into(),
                    workflow_id,
                    failure_mode: PipelineMemberFailureMode::Stop,
                }],
                links: vec![],
                joins: BTreeMap::new(),
            },
            concurrency: Default::default(),
            defaults: Default::default(),
            metadata: Value::Null,
            created_at: None,
            updated_at: None,
        })
        .await
        .unwrap();
    let pipeline_id = pipeline.id.unwrap();

    let org_id = Uuid::now_v7();
    let adapter_id = Uuid::now_v7();
    let (adapter, revision) = db
        .create_orchestration_adapter(
            NewAdapterDefinition {
                id: adapter_id,
                org_id,
                name: format!("parity adapter {suffix}"),
                kind: "generic_webhook".into(),
                kind_version: "1".into(),
                endpoint_identity: format!("parity-{suffix}"),
                configuration: json!({ "authentication": "bearer" }),
                secret_bindings: BTreeMap::new(),
                identity_configuration: json!({ "correlation": "/id" }),
                actor_id: None,
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(adapter.current_revision, 1);
    assert_eq!(revision.revision, 1);

    let admission_id = Uuid::now_v7();
    db.claim_ingress_admission(
        IngressAdmission {
            id: Some(admission_id),
            org_id: Some(org_id),
            scope: format!("parity:{suffix}"),
            correlation_key: "subject".into(),
            generation: 1,
            target: IngressTarget {
                kind: IngressTargetKind::Pipeline,
                id: pipeline_id,
            },
            status: IngressAdmissionStatus::Active,
            workflow_run_id: None,
            pipeline_run_id: None,
            policy: json!({ "scope": "parity", "routes": [] }),
            created_at: now,
            updated_at: now,
        },
        None,
    )
    .await
    .unwrap();

    let event = db
        .record_ingress_event(
            admission_id,
            1,
            IngressEvent {
                source: "adapter:parity".into(),
                event_id: format!("event-{suffix}"),
                event_type: "updated".into(),
                correlation_key: "subject".into(),
                payload: json!({ "revision": "r1" }),
                provenance: json!({ "operation_key": "provider-effect" }),
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();

    let mut policy = OrchestrationPolicy::default();
    policy.intents.insert(
        "stop".into(),
        IntentPolicy {
            effect: ControlEffect::Terminate,
            priority: 100,
            coalesce_seconds: None,
            stop: Default::default(),
            restart: Default::default(),
            subject_revision_pointer: None,
            allow_self_originated: false,
            signal_name: None,
        },
    );
    let binding_id = Uuid::now_v7();
    let new_binding = NewOrchestrationBinding {
        id: binding_id,
        admission_id,
        org_id: Some(org_id),
        scope: format!("parity:{suffix}"),
        correlation_key: "subject".into(),
        generation: 1,
        pipeline_id,
        pipeline_revision: 1,
        pipeline_digest: format!("sha256:{suffix}"),
        adapter_id: Some(adapter_id),
        adapter_revision: Some(1),
        policy,
    };
    let binding = db
        .create_orchestration_binding(new_binding.clone())
        .await
        .unwrap();
    let duplicate = db
        .create_orchestration_binding(NewOrchestrationBinding {
            id: Uuid::now_v7(),
            ..new_binding
        })
        .await
        .unwrap();
    assert_eq!(duplicate.id, binding.id);

    let owner = format!("parity-reducer-{suffix}");
    let claimed = db
        .claim_orchestration_bindings(owner.clone(), now, now + Duration::minutes(1), 100)
        .await
        .unwrap();
    assert!(claimed.iter().any(|candidate| candidate.id == binding_id));
    assert!(
        db.claim_orchestration_bindings(
            format!("parity-rival-{suffix}"),
            now,
            now + Duration::minutes(1),
            100,
        )
        .await
        .unwrap()
        .iter()
        .all(|candidate| candidate.id != binding_id)
    );

    let running = db
        .update_orchestration_binding(
            binding_id,
            owner.clone(),
            OrchestrationBindingUpdate {
                expected_version: 0,
                status: OrchestrationStatus::Running,
                current_phase: Some("member".into()),
                current_attempt: 1,
                current_epoch: 1,
                restart_member: None,
                resume_existing_epoch: false,
                subject_revision: Some("r1".into()),
                resources: json!({ "candidate": "r1" }),
                budgets: BTreeMap::new(),
                last_reduced_sequence: event.entry.sequence,
                finished_at: None,
            },
            now,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(running.version, 1);
    assert!(
        db.update_orchestration_binding(
            binding_id,
            owner.clone(),
            OrchestrationBindingUpdate {
                expected_version: 0,
                status: OrchestrationStatus::Failed,
                current_phase: None,
                current_attempt: 0,
                current_epoch: 0,
                restart_member: None,
                resume_existing_epoch: false,
                subject_revision: None,
                resources: Value::Null,
                budgets: BTreeMap::new(),
                last_reduced_sequence: 0,
                finished_at: Some(now),
            },
            now,
        )
        .await
        .unwrap()
        .is_none()
    );

    let pending = OrchestrationPendingIntent {
        id: Uuid::now_v7(),
        binding_id,
        intent: "stop".into(),
        priority: 100,
        source_event_ids: vec![event.entry.id],
        latest_payload: json!({ "reason": "parity" }),
        wake_at: now,
        created_at: now,
        updated_at: now,
    };
    db.upsert_orchestration_pending_intent(pending.clone())
        .await
        .unwrap();
    assert!(
        db.fetch_due_orchestration_intents(now, 100)
            .await
            .unwrap()
            .iter()
            .any(|candidate| candidate.id == pending.id)
    );
    let consumed = db
        .consume_orchestration_pending_intent(
            binding_id,
            "stop".into(),
            100,
            owner.clone(),
            OrchestrationBindingUpdate {
                expected_version: 1,
                status: OrchestrationStatus::Running,
                current_phase: Some("member".into()),
                current_attempt: 1,
                current_epoch: 1,
                restart_member: None,
                resume_existing_epoch: false,
                subject_revision: Some("r1".into()),
                resources: running.resources,
                budgets: BTreeMap::new(),
                last_reduced_sequence: event.entry.sequence,
                finished_at: None,
            },
            now,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(consumed.version, 2);
    assert!(
        db.fetch_orchestration_pending_intents(binding_id)
            .await
            .unwrap()
            .is_empty()
    );

    let reduction = db
        .record_orchestration_reduction(OrchestrationEventReduction {
            id: Uuid::now_v7(),
            binding_id,
            inbox_event_id: event.entry.id,
            sequence: event.entry.sequence,
            matched_intents: vec!["stop".into()],
            winner: Some("stop".into()),
            suppressed_intents: vec![],
            binding_version: consumed.version,
            disposition: "applied".into(),
            detail: json!({ "dialect": "parity" }),
            created_at: now,
        })
        .await
        .unwrap();
    let duplicate_reduction = db
        .record_orchestration_reduction(OrchestrationEventReduction {
            id: Uuid::now_v7(),
            ..reduction.clone()
        })
        .await
        .unwrap();
    assert_eq!(duplicate_reduction.id, reduction.id);

    let epoch = db
        .create_orchestration_epoch(
            NewOrchestrationEpoch {
                id: Uuid::now_v7(),
                binding_id,
                epoch: 1,
                start_member: Some("member".into()),
                parameters: json!({ "candidate": "r1" }),
                reason: "parity".into(),
            },
            now,
        )
        .await
        .unwrap();
    let duplicate_epoch = db
        .create_orchestration_epoch(
            NewOrchestrationEpoch {
                id: Uuid::now_v7(),
                binding_id,
                epoch: 1,
                start_member: None,
                parameters: Value::Null,
                reason: "duplicate".into(),
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(duplicate_epoch.id, epoch.id);

    let operation_key = format!("start:{suffix}");
    let command = db
        .enqueue_orchestration_command(
            NewOrchestrationCommand {
                id: Uuid::now_v7(),
                binding_id,
                epoch: 1,
                command_type: "start_epoch".into(),
                operation_key: operation_key.clone(),
                payload: Value::Null,
            },
            now,
        )
        .await
        .unwrap();
    let duplicate_command = db
        .enqueue_orchestration_command(
            NewOrchestrationCommand {
                id: Uuid::now_v7(),
                binding_id,
                epoch: 1,
                command_type: "start_epoch".into(),
                operation_key,
                payload: Value::Null,
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(duplicate_command.id, command.id);
    let command_owner = format!("parity-command-{suffix}");
    let claimed_command = db
        .claim_orchestration_commands(command_owner.clone(), now, now + Duration::minutes(1), 100)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == command.id)
        .unwrap();
    assert_eq!(claimed_command.attempts, 1);
    assert!(
        db.complete_orchestration_command(
            command.id,
            command_owner,
            true,
            json!({ "started": true }),
            now,
        )
        .await
        .unwrap()
    );

    let effect_id = Uuid::now_v7();
    let operation = db
        .create_external_operation(ExternalOperation {
            id: Uuid::now_v7(),
            binding_id,
            epoch: 1,
            workflow_run_id: None,
            effect_id: Some(effect_id),
            operation_key: format!("effect:{suffix}"),
            provider: "parity".into(),
            action: "ensure".into(),
            semantics: DeliverySemantics::Reconcilable,
            attempt: 1,
            status: ExternalOperationStatus::Running,
            ambiguous: false,
            provenance: json!({ "operation_key": format!("effect:{suffix}") }),
            receipt: Value::Null,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    assert_eq!(
        db.fetch_external_operation_for_effect(effect_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        operation.id
    );
    let settled = db
        .update_external_operation(
            operation.id,
            ExternalOperationUpdate {
                status: ExternalOperationStatus::Succeeded,
                attempt: 1,
                ambiguous: false,
                provenance: operation.provenance,
                receipt: json!({ "id": "receipt" }),
            },
            now,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.status, ExternalOperationStatus::Succeeded);
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
        node_id: None,
        request: request.clone(),
        status: WorkflowEffectStatus::Requested,
        current_executor_replica_id: None,
        last_executor_replica_id: None,
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
        executor: runinator_comm::EffectExecutor::Infrastructure,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: effect.idempotency_key(),
        notification_delivery_id: None,
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

    // the executor lease. a claim marks the effect running and names the replica; settling releases
    // the claim but keeps the attribution, which is what `count_running_effects_by_executor` and
    // the stale-replica reaper read now that node runs are gone.
    let replica_id = Uuid::now_v7();
    assert!(
        db.claim_workflow_effect_executor(effect_id, 0, replica_id, Utc::now())
            .await
            .unwrap()
    );
    let claimed_effect = db
        .fetch_workflow_effect(effect_id)
        .await
        .unwrap()
        .expect("effect");
    assert_eq!(claimed_effect.status, WorkflowEffectStatus::Running);
    assert_eq!(claimed_effect.current_executor_replica_id, Some(replica_id));
    assert_eq!(
        db.count_running_effects_by_executor().await.unwrap(),
        vec![(replica_id, 1)]
    );
    // a stale attempt must not steal the lease from the live executor.
    assert!(
        !db.claim_workflow_effect_executor(effect_id, 1, Uuid::now_v7(), Utc::now())
            .await
            .unwrap()
    );
    assert!(
        db.settle_workflow_effect(
            effect_id,
            0,
            WorkflowEffectStatus::Succeeded,
            None,
            None,
            Utc::now(),
        )
        .await
        .unwrap()
    );
    // the engine's action deadline backstop settles `TimedOut` from a timer wake without first
    // reading the effect, so losing the race to a real result must be an exact no-op. this guard is
    // what makes the backstop safe to arm beside every dispatched action.
    assert!(
        !db.settle_workflow_effect(
            effect_id,
            0,
            WorkflowEffectStatus::TimedOut,
            None,
            Some("no result within 60s; the executing worker never reported".into()),
            Utc::now(),
        )
        .await
        .unwrap()
    );
    let settled_effect = db
        .fetch_workflow_effect(effect_id)
        .await
        .unwrap()
        .expect("effect");
    assert_eq!(settled_effect.status, WorkflowEffectStatus::Succeeded);
    assert_eq!(settled_effect.message, None);
    assert_eq!(settled_effect.current_executor_replica_id, None);
    assert_eq!(settled_effect.last_executor_replica_id, Some(replica_id));
    assert!(
        db.count_running_effects_by_executor()
            .await
            .unwrap()
            .is_empty()
    );

    // deleting the run must take its whole vm footprint with it. the delete names every child
    // explicitly rather than trusting the declared cascades, because mysql 8 discards a
    // column-level `REFERENCES` that mariadb honours — so a cascade-only delete orphans rows on one
    // engine and not the other. asserting the read-backs here is what proves the statement list is
    // complete on all three.
    db.delete_workflow_run(run.id).await.unwrap();
    assert!(db.fetch_workflow_run(run.id).await.unwrap().is_none());
    assert!(db.fetch_workflow_module(run.id).await.unwrap().is_none());
    assert!(
        db.fetch_workflow_continuations(run.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(db.fetch_workflow_effects(run.id).await.unwrap().is_empty());
    assert!(db.fetch_workflow_effect(effect_id).await.unwrap().is_none());
    assert!(db.fetch_workflow_journal(run.id).await.unwrap().is_empty());
}

/// A named mutex must come back when its holder finishes. The release lived on the removed reducer
/// store, so after the VM cutover a key acquired by one run stayed locked forever and every later
/// run of the same key spun in the infrastructure host's acquire loop.
async fn assert_workflow_vm_mutex_lifecycle<T: DatabaseImpl + WorkflowVmStore>(
    db: &T,
    workflow: &WorkflowDefinition,
) {
    async fn start_run<T: DatabaseImpl>(db: &T, workflow_id: Uuid) -> Uuid {
        let snapshot = db
            .fetch_workflow(workflow_id)
            .await
            .unwrap()
            .expect("workflow snapshot");
        db.create_workflow_run(
            workflow_id,
            snapshot,
            Value::Null,
            Value::Null,
            None,
            Default::default(),
        )
        .await
        .unwrap()
        .id
    }
    let workflow_id = workflow.id.expect("workflow id");
    let holder = start_run(db, workflow_id).await;
    let waiter = start_run(db, workflow_id).await;
    let key = format!("parity-mutex-{}", Uuid::now_v7());
    let now = Utc::now().timestamp();
    let holder_continuation = Uuid::now_v7();
    let waiter_continuation = Uuid::now_v7();

    assert!(
        db.claim_workflow_vm_mutex(key.clone(), holder, holder_continuation, now)
            .await
            .unwrap()
    );
    // re-entrant for the holder, refused for anyone else.
    assert!(
        db.claim_workflow_vm_mutex(key.clone(), holder, holder_continuation, now)
            .await
            .unwrap()
    );
    assert!(
        !db.claim_workflow_vm_mutex(key.clone(), waiter, waiter_continuation, now)
            .await
            .unwrap()
    );

    // A bracketed mutex releases before the enclosing run finishes. Without this path the
    // generated `mutex release` node behaves like a second acquire and blocks every waiter until
    // the entire workflow exits.
    db.release_workflow_vm_mutex(key.clone(), holder, holder_continuation, now)
        .await
        .unwrap();
    assert!(
        db.claim_workflow_vm_mutex(key.clone(), waiter, waiter_continuation, now)
            .await
            .unwrap(),
        "an explicit release must admit the waiter before the holder run settles"
    );

    db.settle_workflow_vm_run(holder, WorkflowStatus::Succeeded, None)
        .await
        .unwrap();

    // The implicit release at terminal run settlement remains the backstop for acquire-only
    // mutexes, which have no matching release node.
    let terminal_successor = start_run(db, workflow_id).await;
    db.settle_workflow_vm_run(waiter, WorkflowStatus::Succeeded, None)
        .await
        .unwrap();
    assert!(
        db.claim_workflow_vm_mutex(key.clone(), terminal_successor, Uuid::now_v7(), now)
            .await
            .unwrap(),
        "settling the holder must release the key"
    );

    // And a holder that reached a terminal without releasing — a crash between the two writes —
    // is treated as stale rather than deadlocking the key.
    let recovery = start_run(db, workflow_id).await;
    db.update_workflow_run_status(terminal_successor, WorkflowStatus::Failed, None, None, None)
        .await
        .unwrap();
    assert!(
        db.claim_workflow_vm_mutex(key.clone(), recovery, Uuid::now_v7(), now)
            .await
            .unwrap(),
        "a terminal holder must not hold the key forever"
    );
    db.cancel_workflow_vm_run(recovery, "parity teardown".into())
        .await
        .unwrap();
    for run in [holder, waiter, terminal_successor, recovery] {
        db.delete_workflow_run(run).await.unwrap();
    }
}

async fn assert_agent_directive_lifecycle<T: DatabaseImpl>(db: &T) {
    let replica = db
        .register_replica(
            runinator_models::replicas::ReplicaRegistrationRequest {
                replica_id: None,
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
        digest: WorkflowRevision::content_digest(
            workflow.version,
            &workflow.input_type,
            &workflow.definition,
        ),
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
        .claim_due_workflow_trigger_firings(
            "parity".to_string(),
            now,
            10,
            std::collections::HashMap::from([(
                workflow_id,
                runinator_store::roles::ScheduledWorkflowVm {
                    snapshot: workflow.clone(),
                    module: WorkflowModule::new(vec![WorkflowInstruction::Return]),
                },
            )]),
        )
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

async fn assert_normalized_execution_state_lifecycle<T: DatabaseImpl + WorkflowVmStore>(
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

async fn assert_ingress_admission_claim<T: DatabaseImpl>(db: &T) {
    let admission = IngressAdmission {
        id: None,
        org_id: None,
        scope: "release.lifecycle".into(),
        correlation_key: "release-42".into(),
        generation: 1,
        target: IngressTarget {
            kind: IngressTargetKind::Pipeline,
            id: Uuid::now_v7(),
        },
        status: IngressAdmissionStatus::Active,
        workflow_run_id: None,
        pipeline_run_id: None,
        policy: json!({ "scope": "release.lifecycle", "routes": [] }),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let first = db
        .claim_ingress_admission(admission.clone(), None)
        .await
        .unwrap();
    let saved = match first {
        IngressAdmissionClaim::Acquired(saved) => saved,
        IngressAdmissionClaim::Existing(_) => panic!("first ingress claim must acquire"),
    };
    let second = db.claim_ingress_admission(admission, None).await.unwrap();
    let existing = match second {
        IngressAdmissionClaim::Existing(existing) => existing,
        IngressAdmissionClaim::Acquired(_) => panic!("second ingress claim must be rejected"),
    };
    assert_eq!(existing.id, saved.id);
    assert_eq!(existing.target.kind, IngressTargetKind::Pipeline);
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
    use runinator_models::console::{
        ConsoleCellKind, ConsoleCellStatus, NewConsoleCell, NewConsoleFunction,
    };

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

    // The active function library is latest-successful by name. Changing a non-owner must not
    // disturb the latest definition, while editing or deleting its owner removes it outright.
    let definition = |source: &str| NewConsoleFunction {
        name: "double".into(),
        is_task: false,
        source: source.into(),
    };
    db.replace_console_functions(
        session.id,
        first.id,
        &[definition("fn double(x: integer) = x * 2")],
    )
    .await
    .unwrap();
    db.replace_console_functions(
        session.id,
        second.id,
        &[definition("fn double(x: integer) = x * 3")],
    )
    .await
    .unwrap();
    let active = db.fetch_console_functions(session.id).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].cell_id, second.id);
    assert!(active[0].source.contains("* 3"));

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
    assert_eq!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .first()
            .map(|function| function.cell_id),
        Some(second.id),
        "editing an older owner must not revive or remove the newer definition"
    );

    db.upsert_console_cell(
        session.id,
        Some(second.id),
        &NewConsoleCell {
            source: "console.run(command: \"new\")".into(),
            label: None,
            position: None,
        },
    )
    .await
    .unwrap();
    assert!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .is_empty()
    );

    db.replace_console_functions(
        session.id,
        first.id,
        &[definition("fn double(x: integer) = x * 4")],
    )
    .await
    .unwrap();

    // A stale completion must not overwrite an edit or republish the function source that ran
    // before it. Editing clears the run id; the conditional terminal transition observes that.
    let stale_run_id = Uuid::new_v4();
    db.record_console_cell_outcome(
        second.id,
        Some(ConsoleCellKind::Workflow),
        ConsoleCellStatus::Running,
        None,
        None,
        Some(stale_run_id),
    )
    .await
    .unwrap();
    db.upsert_console_cell(
        session.id,
        Some(second.id),
        &NewConsoleCell {
            source: "console.run(command: \"edited\")".into(),
            label: None,
            position: None,
        },
    )
    .await
    .unwrap();
    assert!(
        db.settle_console_workflow_succeeded(
            second.id,
            stale_run_id,
            "cell_1",
            &runinator_models::json!("stale"),
            &[definition("fn double(x: integer) = x * 999")],
        )
        .await
        .unwrap()
        .is_none(),
        "a completion that no longer owns the cell is ignored"
    );
    assert_eq!(
        db.fetch_console_cell(second.id)
            .await
            .unwrap()
            .map(|cell| cell.status),
        Some(ConsoleCellStatus::Idle)
    );
    assert!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .iter()
            .any(|function| function.source.contains("* 4")),
        "the stale source did not replace the active definition"
    );

    // A current completion moves the result binding, terminal state, and declarations together.
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
    let settled = db
        .settle_console_workflow_succeeded(
            second.id,
            run_id,
            "cell_1",
            &runinator_models::json!("done"),
            &[definition("fn double(x: integer) = x * 5")],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(settled.status, ConsoleCellStatus::Succeeded);
    assert_eq!(settled.result, Some(runinator_models::json!("done")));
    assert_eq!(
        db.fetch_console_cell_for_run(run_id)
            .await
            .unwrap()
            .map(|cell| cell.id),
        Some(second.id)
    );
    assert_eq!(
        db.fetch_console_bindings(session.id)
            .await
            .unwrap()
            .iter()
            .find(|binding| binding.name == "cell_1")
            .map(|binding| binding.value.clone()),
        Some(runinator_models::json!("done"))
    );
    assert_eq!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .first()
            .map(|function| function.cell_id),
        Some(second.id)
    );

    // A later failed replay only clears the result binding it owned. It neither republishes nor
    // removes the definition from the last successful execution.
    let failed_run_id = Uuid::new_v4();
    db.record_console_cell_outcome(
        second.id,
        Some(ConsoleCellKind::Workflow),
        ConsoleCellStatus::Running,
        None,
        None,
        Some(failed_run_id),
    )
    .await
    .unwrap();
    let failed = db
        .settle_console_workflow_failed(second.id, failed_run_id, "cell_1", "provider failed")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, ConsoleCellStatus::Failed);
    assert!(failed.result.is_none());
    assert!(
        db.fetch_console_bindings(session.id)
            .await
            .unwrap()
            .iter()
            .all(|binding| binding.name != "cell_1")
    );
    assert!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .iter()
            .any(|function| function.source.contains("* 5")),
        "a failed replay does not alter the last successful publication"
    );

    // deleting a cell takes its binding with it.
    assert!(db.delete_console_cell(first.id).await.unwrap());
    assert!(
        db.fetch_console_bindings(session.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(db.delete_console_cell(second.id).await.unwrap());
    assert!(
        db.fetch_console_bindings(session.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .is_empty()
    );

    // Function-only cells publish their terminal outcome and library entries together, and the
    // source compare prevents validation that began before an edit from reviving removed entries.
    let library_source = "fn triple(x: integer) = x * 3";
    let library = db
        .upsert_console_cell(
            session.id,
            None,
            &NewConsoleCell {
                source: library_source.into(),
                label: None,
                position: None,
            },
        )
        .await
        .unwrap();
    let published = db
        .publish_console_library_cell(
            library.id,
            library_source,
            &[NewConsoleFunction {
                name: "triple".into(),
                is_task: false,
                source: library_source.into(),
            }],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(published.kind, Some(ConsoleCellKind::Library));
    assert_eq!(published.status, ConsoleCellStatus::Succeeded);
    assert_eq!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .first()
            .map(|function| function.name.as_str()),
        Some("triple")
    );
    db.upsert_console_cell(
        session.id,
        Some(library.id),
        &NewConsoleCell {
            source: "fn triple(x: integer) = x * 30".into(),
            label: None,
            position: None,
        },
    )
    .await
    .unwrap();
    assert!(
        db.publish_console_library_cell(
            library.id,
            library_source,
            &[NewConsoleFunction {
                name: "triple".into(),
                is_task: false,
                source: library_source.into(),
            }],
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .is_empty()
    );

    // Clearing keeps the named session, but fully resets its notebook and durable scope. This is
    // deliberately stronger than clearing a terminal transcript: subsequent cells must not see a
    // value or function from before the reset.
    let reset_cell = db
        .upsert_console_cell(
            session.id,
            None,
            &NewConsoleCell {
                source: "41 + 1".into(),
                label: Some("answer".into()),
                position: None,
            },
        )
        .await
        .unwrap();
    db.upsert_console_binding(
        session.id,
        "answer",
        Some(reset_cell.id),
        &runinator_models::json!(42),
    )
    .await
    .unwrap();
    db.replace_console_functions(
        session.id,
        reset_cell.id,
        &[NewConsoleFunction {
            name: "answer".into(),
            is_task: false,
            source: "fn answer() = 42".into(),
        }],
    )
    .await
    .unwrap();
    assert!(db.clear_console_session(session.id).await.unwrap());
    assert!(
        db.fetch_console_session(session.id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(db.fetch_console_cells(session.id).await.unwrap().is_empty());
    assert!(
        db.fetch_console_bindings(session.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.fetch_console_functions(session.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!db.clear_console_session(Uuid::new_v4()).await.unwrap());

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

/// Re-arming a failed effect: the attempt advances, the outcome is cleared, the continuation stays
/// parked, and a *delayed* dispatch is queued that the publisher cannot claim before it is due.
async fn assert_workflow_effect_retry_lifecycle<T: DatabaseImpl + WorkflowVmStore>(
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
    let claimed = db
        .claim_runnable_workflow_continuations(
            format!("retry-parity-{}", Uuid::now_v7()),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            10,
        )
        .await
        .unwrap();
    let claimed = claimed
        .into_iter()
        .find(|continuation| continuation.id == root.id)
        .expect("the new run's root continuation is claimable");
    let runinator_runtime::WorkflowVmStep::Yield {
        continuation,
        effect_id,
        sequence,
        request,
    } = runinator_runtime::step_workflow_vm(&module, claimed)
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
        node_id: None,
        request: request.clone(),
        status: WorkflowEffectStatus::Requested,
        current_executor_replica_id: None,
        last_executor_replica_id: None,
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
        executor: runinator_comm::EffectExecutor::Provider,
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        idempotency_key: effect.idempotency_key(),
        notification_delivery_id: None,
    };
    let continuation_id = continuation.id;
    db.suspend_on_effect(continuation, effect, command)
        .await
        .unwrap();
    let replica_id = Uuid::now_v7();
    assert!(
        db.claim_workflow_effect_executor(effect_id, 0, replica_id, Utc::now())
            .await
            .unwrap()
    );

    let due = Utc::now() + Duration::seconds(120);
    assert!(
        db.retry_workflow_effect(effect_id, 0, due, Some("boom".into()), Utc::now())
            .await
            .unwrap()
    );
    let retried = db
        .fetch_workflow_effect(effect_id)
        .await
        .unwrap()
        .expect("effect");
    assert_eq!(retried.attempt, 1);
    assert_eq!(retried.status, WorkflowEffectStatus::Requested);
    assert_eq!(retried.result, None);
    assert_eq!(retried.finished_at, None);
    // the lease is released but the attribution survives, exactly as settling does.
    assert_eq!(retried.current_executor_replica_id, None);
    assert_eq!(retried.last_executor_replica_id, Some(replica_id));
    // the parked thread must stay waiting: a retry is invisible to the graph.
    let parked = db
        .fetch_workflow_continuation(continuation_id)
        .await
        .unwrap()
        .expect("continuation");
    assert_eq!(
        parked.status,
        runinator_models::workflow_vm::WorkflowContinuationStatus::Waiting
    );

    // the same result arriving twice must not schedule a second attempt.
    assert!(
        !db.retry_workflow_effect(effect_id, 0, due, None, Utc::now())
            .await
            .unwrap()
    );

    // the re-dispatch is not claimable until it is due, and carries the bumped attempt when it is.
    let publisher = format!("retry-publisher-{}", Uuid::now_v7());
    let early = db
        .claim_pending_workflow_effect_dispatches(
            publisher.clone(),
            Utc::now(),
            Utc::now() + Duration::seconds(30),
            50,
        )
        .await
        .unwrap();
    // the original attempt-0 dispatch is still unpublished here, so key on the attempt: it is the
    // retry specifically that must not be claimable yet.
    assert!(
        !early
            .iter()
            .any(|record| record.effect_id == effect_id && record.command.attempt == 1),
        "a retry must not be published before its backoff has elapsed"
    );
    let ready = db
        .claim_pending_workflow_effect_dispatches(
            publisher,
            due + Duration::seconds(1),
            due + Duration::seconds(31),
            50,
        )
        .await
        .unwrap();
    let record = ready
        .iter()
        .find(|record| record.effect_id == effect_id && record.command.attempt == 1)
        .expect("the retry becomes claimable once due");
    assert_eq!(
        record.command.idempotency_key,
        retried.idempotency_key(),
        "the attempt is part of the key, so the worker cannot replay the failed attempt"
    );
}
