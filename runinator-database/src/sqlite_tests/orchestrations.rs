use std::collections::BTreeMap;

use super::*;
use runinator_models::{
    orchestration::{
        ControlEffect, DeliverySemantics, ExternalOperation, ExternalOperationStatus,
        IngressAdmission, IngressAdmissionStatus, IngressTarget, IngressTargetKind, IntentPolicy,
        NewOrchestrationBinding, OrchestrationPendingIntent, OrchestrationPolicy,
        OrchestrationStatus,
    },
    pipelines::{
        Pipeline, PipelineExecutionContext, PipelineGraph, PipelineMember,
        PipelineMemberFailureMode,
    },
    workflow_vm::{WorkflowInstruction, WorkflowModule},
};
use runinator_store::roles::{
    ExternalOperationUpdate, NewAdapterDefinition, NewAdapterRevision, NewOrchestrationCommand,
    NewOrchestrationEpoch, NewWorkflowVmRun, OrchestrationBindingUpdate,
};

#[tokio::test]
async fn standalone_workflow_run_has_no_orchestration_binding() {
    let path = std::env::temp_dir().join(format!(
        "runinator-standalone-orchestration-{}.db",
        Uuid::now_v7()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let snapshot = db.upsert_workflow(&workflow("standalone")).await.unwrap();
    let workflow_id = snapshot.id.unwrap();
    let run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            replay_seed: None,
            workflow_id,
            workflow_snapshot: snapshot,
            parameters: Value::Null,
            config: Value::Null,
            state: Value::Null,
            name: None,
            provenance: Default::default(),
            pipeline_run_id: None,
            pipeline_member_attempt_id: None,
            module: WorkflowModule::new(vec![WorkflowInstruction::Return]),
            instruction_pointer: 0,
        })
        .await
        .unwrap();

    assert!(
        db.fetch_current_orchestration_binding_for_workflow_run(run.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn orchestration_binding_lease_cas_epoch_and_command_outbox_are_durable() {
    let path = std::env::temp_dir().join(format!("runinator-orchestration-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let now = Utc::now();

    let member = db.upsert_workflow(&workflow("member")).await.unwrap();
    let pipeline = db
        .upsert_pipeline(&Pipeline {
            id: None,
            name: "Correlated work".into(),
            key: Some("correlated_work".into()),
            namespace: Some("tests".into()),
            description: None,
            org_id: None,
            enabled: true,
            graph: PipelineGraph {
                version: runinator_models::pipelines::PIPELINE_GRAPH_VERSION,
                members: vec![PipelineMember {
                    workspace: None,
                    key: "member".into(),
                    workflow_id: member.id.unwrap(),
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

    let adapter_id = Uuid::now_v7();
    let (adapter, adapter_revision) = db
        .create_orchestration_adapter(
            NewAdapterDefinition {
                id: adapter_id,
                org_id: Uuid::now_v7(),
                name: "Webhook".into(),
                kind: "generic_webhook".into(),
                kind_version: "1".into(),
                transport: runinator_models::orchestration::AdapterTransport::Webhook,
                endpoint_identity: "endpoint-token".into(),
                configuration: runinator_models::json!({ "authentication": "bearer" }),
                secret_bindings: BTreeMap::new(),
                identity_configuration: runinator_models::json!({ "correlation": "/id" }),
                actor_id: None,
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(adapter.current_revision, 1);
    assert_eq!(adapter_revision.revision, 1);

    let admission_id = Uuid::now_v7();
    db.claim_ingress_admission(
        IngressAdmission {
            id: Some(admission_id),
            org_id: None,
            scope: "objects".into(),
            correlation_key: "object-7".into(),
            generation: 1,
            target: IngressTarget {
                kind: IngressTargetKind::Pipeline,
                id: pipeline_id,
            },
            status: IngressAdmissionStatus::Active,
            workflow_run_id: None,
            pipeline_run_id: None,
            policy: runinator_models::json!({ "scope": "objects", "routes": [] }),
            created_at: now,
            updated_at: now,
        },
        None,
    )
    .await
    .unwrap();

    let mut policy = OrchestrationPolicy::default();
    policy.intents.insert(
        "stop".into(),
        IntentPolicy {
            priority: 100,
            effect: ControlEffect::Terminate,
            coalesce_seconds: None,
            stop: Default::default(),
            restart: Default::default(),
            subject_revision_pointer: None,
            allow_self_originated: false,
            signal_name: None,
        },
    );
    let binding_id = Uuid::now_v7();
    let binding = db
        .create_orchestration_binding(NewOrchestrationBinding {
            id: binding_id,
            admission_id,
            org_id: None,
            scope: "objects".into(),
            correlation_key: "object-7".into(),
            generation: 1,
            pipeline_id,
            pipeline_revision: 3,
            pipeline_digest: "sha256:test".into(),
            adapter_id: Some(adapter_id),
            adapter_revision: Some(1),
            policy,
        })
        .await
        .unwrap();
    assert_eq!(binding.status, OrchestrationStatus::Pending);
    assert_eq!(binding.version, 0);

    let ingress_event = db
        .record_ingress_event(
            admission_id,
            1,
            runinator_models::orchestration::IngressEvent {
                source: "adapter:test".into(),
                event_id: "event-with-provenance".into(),
                event_type: "updated".into(),
                correlation_key: "object-7".into(),
                payload: runinator_models::json!({ "value": 1 }),
                provenance: runinator_models::json!({ "operation_key": "operation-7" }),
                occurred_at: Some(now),
            },
            runinator_models::orchestration::IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
    assert_eq!(
        ingress_event
            .entry
            .provenance
            .get("operation_key")
            .and_then(Value::as_str),
        Some("operation-7")
    );

    let duplicate = db
        .create_orchestration_binding(NewOrchestrationBinding {
            id: Uuid::now_v7(),
            admission_id,
            org_id: None,
            scope: "objects".into(),
            correlation_key: "object-7".into(),
            generation: 1,
            pipeline_id,
            pipeline_revision: 3,
            pipeline_digest: "sha256:test".into(),
            adapter_id: Some(adapter_id),
            adapter_revision: Some(1),
            policy: binding.policy.clone(),
        })
        .await
        .unwrap();
    assert_eq!(duplicate.id, binding_id);

    let claimed = db
        .claim_orchestration_bindings("reducer-a".into(), now, now + Duration::minutes(1), 10)
        .await
        .unwrap();
    assert_eq!(
        claimed.iter().map(|binding| binding.id).collect::<Vec<_>>(),
        vec![binding_id]
    );
    assert!(
        db.claim_orchestration_bindings("reducer-b".into(), now, now + Duration::minutes(1), 10,)
            .await
            .unwrap()
            .is_empty()
    );

    let update = OrchestrationBindingUpdate {
        expected_version: 0,
        status: OrchestrationStatus::Running,
        current_phase: Some("member".into()),
        current_attempt: 1,
        current_epoch: 1,
        restart_member: None,
        resume_existing_epoch: false,
        subject_revision: Some("r1".into()),
        resources: runinator_models::json!({ "candidate": "r1" }),
        budgets: BTreeMap::new(),
        last_reduced_sequence: ingress_event.entry.sequence,
        finished_at: None,
    };
    let updated = db
        .update_orchestration_binding(binding_id, "reducer-a".into(), update.clone(), now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.version, 1);
    assert!(
        db.update_orchestration_binding(binding_id, "reducer-a".into(), update, now)
            .await
            .unwrap()
            .is_none()
    );

    let pipeline_run = db
        .create_pipeline_run(
            pipeline_id,
            pipeline.clone(),
            Value::Null,
            Value::Null,
            Default::default(),
            PipelineExecutionContext {
                orchestration_binding_id: Some(binding_id),
                execution_epoch: Some(1),
                start_member: None,
            },
        )
        .await
        .unwrap();
    let workflow_id = member.id.unwrap();
    let attempt = db
        .create_pipeline_member_attempt(
            pipeline_run.id,
            "member".into(),
            workflow_id,
            1,
            Value::Null,
        )
        .await
        .unwrap()
        .unwrap();
    let workflow_run = db
        .create_workflow_vm_run(NewWorkflowVmRun {
            replay_seed: None,
            workflow_id,
            workflow_snapshot: member,
            parameters: Value::Null,
            config: Value::Null,
            state: Value::Null,
            name: None,
            provenance: Default::default(),
            pipeline_run_id: Some(pipeline_run.id),
            pipeline_member_attempt_id: Some(attempt.id),
            module: WorkflowModule::new(vec![WorkflowInstruction::Return]),
            instruction_pointer: 0,
        })
        .await
        .unwrap();
    assert_eq!(
        db.fetch_current_orchestration_binding_for_workflow_run(workflow_run.id)
            .await
            .unwrap()
            .unwrap()
            .id,
        binding_id
    );

    db.upsert_orchestration_pending_intent(OrchestrationPendingIntent {
        id: Uuid::now_v7(),
        binding_id,
        intent: "pause".into(),
        priority: 60,
        source_event_ids: vec![ingress_event.entry.id],
        latest_payload: runinator_models::json!({ "reason": "test" }),
        wake_at: now,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();
    db.upsert_orchestration_pending_intent(OrchestrationPendingIntent {
        id: Uuid::now_v7(),
        binding_id,
        intent: "audit".into(),
        priority: 10,
        source_event_ids: vec![ingress_event.entry.id],
        latest_payload: Value::Null,
        wake_at: now,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();
    let pending_update = OrchestrationBindingUpdate {
        expected_version: 0,
        status: OrchestrationStatus::Suspended,
        current_phase: Some("member".into()),
        current_attempt: 1,
        current_epoch: 1,
        restart_member: Some("member".into()),
        resume_existing_epoch: false,
        subject_revision: Some("r1".into()),
        resources: runinator_models::json!({ "candidate": "r1" }),
        budgets: BTreeMap::new(),
        last_reduced_sequence: ingress_event.entry.sequence,
        finished_at: None,
    };
    assert!(
        db.consume_orchestration_pending_intent(
            binding_id,
            "pause".into(),
            60,
            "reducer-a".into(),
            pending_update.clone(),
            now,
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(
        db.fetch_orchestration_pending_intents(binding_id)
            .await
            .unwrap()
            .len(),
        2
    );
    let consumed = db
        .consume_orchestration_pending_intent(
            binding_id,
            "pause".into(),
            60,
            "reducer-a".into(),
            OrchestrationBindingUpdate {
                expected_version: 1,
                ..pending_update
            },
            now,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(consumed.version, 2);
    assert_eq!(consumed.status, OrchestrationStatus::Suspended);
    assert!(
        db.fetch_orchestration_pending_intents(binding_id)
            .await
            .unwrap()
            .is_empty()
    );

    let epoch = db
        .create_orchestration_epoch(
            NewOrchestrationEpoch {
                id: Uuid::now_v7(),
                binding_id,
                epoch: 1,
                start_member: Some("member".into()),
                parameters: runinator_models::json!({ "candidate": "r1" }),
                reason: "initial".into(),
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
                start_member: Some("member".into()),
                parameters: Value::Null,
                reason: "duplicate".into(),
            },
            now,
        )
        .await
        .unwrap();
    assert_eq!(epoch.id, duplicate_epoch.id);
    assert!(
        db.settle_orchestration_epoch(binding_id, 1, "failed".into(), now)
            .await
            .unwrap()
    );
    assert!(
        !db.settle_orchestration_epoch(binding_id, 1, "succeeded".into(), now)
            .await
            .unwrap()
    );
    let settled_epoch = db
        .fetch_orchestration_epochs(binding_id)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.epoch == 1)
        .unwrap();
    assert_eq!(settled_epoch.status, "failed");
    assert!(settled_epoch.finished_at.is_some());

    let operation_key = "binding:start:1".to_string();
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
    assert_eq!(command.id, duplicate_command.id);
    let claimed_command = db
        .claim_orchestration_commands("command-a".into(), now, now + Duration::minutes(1), 10)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == command.id)
        .unwrap();
    assert_eq!(claimed_command.attempts, 1);
    assert!(
        db.retry_orchestration_command(
            command.id,
            "command-a".into(),
            runinator_models::json!({ "error": "transient" }),
            now,
        )
        .await
        .unwrap()
    );
    let retried_command = db
        .claim_orchestration_commands("command-b".into(), now, now + Duration::minutes(1), 10)
        .await
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == command.id)
        .unwrap();
    assert_eq!(retried_command.attempts, 2);
    assert!(
        db.complete_orchestration_command(command.id, "command-b".into(), true, Value::Null, now,)
            .await
            .unwrap()
    );

    assert!(
        db.mark_orchestration_adapter_admitted(adapter_id, now)
            .await
            .unwrap()
    );
    let changed_identity = db
        .create_orchestration_adapter_revision(
            NewAdapterRevision {
                id: Uuid::now_v7(),
                adapter_id,
                expected_revision: 1,
                kind_version: "1".into(),
                transport: runinator_models::orchestration::AdapterTransport::Webhook,
                configuration: Value::Null,
                secret_bindings: BTreeMap::new(),
                identity_configuration: runinator_models::json!({ "correlation": "/other" }),
                actor_id: None,
            },
            now,
        )
        .await;
    assert!(changed_identity.is_err());

    let operation_id = Uuid::now_v7();
    let operation = db
        .create_external_operation(ExternalOperation {
            id: operation_id,
            binding_id,
            epoch: 1,
            workflow_run_id: Some(Uuid::now_v7()),
            effect_id: Some(Uuid::now_v7()),
            operation_key: "jira:comment:review".into(),
            provider: "jira".into(),
            action: "ensure_comment".into(),
            semantics: DeliverySemantics::Reconcilable,
            attempt: 1,
            status: ExternalOperationStatus::Running,
            ambiguous: false,
            provenance: runinator_models::json!({ "marker": "op-1" }),
            receipt: Value::Null,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    let duplicate_operation = db
        .create_external_operation(ExternalOperation {
            id: Uuid::now_v7(),
            ..operation.clone()
        })
        .await
        .unwrap();
    assert_eq!(duplicate_operation.id, operation_id);
    let succeeded = db
        .update_external_operation(
            operation_id,
            ExternalOperationUpdate {
                status: ExternalOperationStatus::Succeeded,
                attempt: 1,
                ambiguous: false,
                provenance: operation.provenance,
                receipt: runinator_models::json!({ "id": "comment-1" }),
            },
            now,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(succeeded.status, ExternalOperationStatus::Succeeded);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn polling_adapter_claim_checkpoint_and_transport_switch_are_durable() {
    use runinator_models::orchestration::AdapterTransport;

    let path = std::env::temp_dir().join(format!("runinator-adapter-poll-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let now = Utc::now();
    let adapter_id = Uuid::now_v7();
    db.create_orchestration_adapter(
        NewAdapterDefinition {
            id: adapter_id,
            org_id: Uuid::now_v7(),
            name: "GitHub poll".into(),
            kind: "github".into(),
            kind_version: "1".into(),
            transport: AdapterTransport::Polling,
            endpoint_identity: "poll-endpoint".into(),
            configuration: runinator_models::json!({"repositories": ["acme/repo"]}),
            secret_bindings: BTreeMap::new(),
            identity_configuration: Value::Null,
            actor_id: None,
        },
        now,
    )
    .await
    .unwrap();

    let initial = db
        .fetch_orchestration_adapter_poll_status(adapter_id)
        .await
        .unwrap()
        .unwrap();
    assert!(initial.checkpoint.is_null());
    let claims = db
        .claim_due_orchestration_adapter_polls(
            "engine-a".into(),
            now,
            now + chrono::TimeDelta::seconds(60),
            10,
        )
        .await
        .unwrap();
    assert_eq!(claims.len(), 1);
    assert!(
        db.claim_due_orchestration_adapter_polls(
            "engine-b".into(),
            now,
            now + chrono::TimeDelta::seconds(60),
            10
        )
        .await
        .unwrap()
        .is_empty()
    );
    assert!(
        db.complete_orchestration_adapter_poll(
            adapter_id,
            "engine-a".into(),
            1,
            runinator_models::json!({"updated_at": "2026-08-27T00:00:00Z"}),
            now + chrono::TimeDelta::seconds(60),
            now
        )
        .await
        .unwrap()
    );
    let completed = db
        .fetch_orchestration_adapter_poll_status(adapter_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        completed
            .checkpoint
            .get("updated_at")
            .and_then(|value| value.as_str()),
        Some("2026-08-27T00:00:00Z")
    );

    db.create_orchestration_adapter_revision(
        NewAdapterRevision {
            id: Uuid::now_v7(),
            adapter_id,
            expected_revision: 1,
            kind_version: "1".into(),
            transport: AdapterTransport::Webhook,
            configuration: Value::Null,
            secret_bindings: BTreeMap::new(),
            identity_configuration: Value::Null,
            actor_id: None,
        },
        now,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        db.fetch_orchestration_adapter_poll_status(adapter_id)
            .await
            .unwrap()
            .is_none()
    );

    let _ = std::fs::remove_file(path);
}
