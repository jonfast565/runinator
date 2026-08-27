use std::collections::BTreeMap;

use super::*;
use runinator_models::{
    orchestration::{
        ControlEffect, IngressAdmission, IngressAdmissionStatus, IngressTarget, IngressTargetKind,
        IntentPolicy, NewOrchestrationBinding, OrchestrationPolicy, OrchestrationStatus,
    },
    pipelines::{Pipeline, PipelineGraph, PipelineMember, PipelineMemberFailureMode},
};
use runinator_store::roles::{
    NewOrchestrationCommand, NewOrchestrationEpoch, OrchestrationBindingUpdate,
};

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
            graph: PipelineGraph {
                version: runinator_models::pipelines::PIPELINE_GRAPH_VERSION,
                members: vec![PipelineMember {
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
            policy,
        })
        .await
        .unwrap();
    assert_eq!(binding.status, OrchestrationStatus::Pending);
    assert_eq!(binding.version, 0);

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
        last_reduced_sequence: 1,
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

    let _ = std::fs::remove_file(path);
}
