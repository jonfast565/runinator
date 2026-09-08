//! One regression for durable capture, worker dry-runs, and preview/live preparation parity.
use super::*;
use crate::services::{AdapterOperations, PipelineOperations};
use chrono::{Duration, Utc};
use runinator_adapter_contract::{AdapterPollRequest, AdapterPollResponse};
use runinator_broker_core::{Broker, UiEventPublisher, in_memory::InMemoryBroker};
use runinator_comm::EffectResultKind;
use runinator_models::{
    auth::ResourceType,
    execution_profiles::ExecutionProfileBinding,
    orchestration::*,
    rbac::{ResourceOwnership, ScopeRef},
    value::Value,
    workflow_vm::WorkflowEffectStatus,
};
use runinator_store::prelude::*;
use runinator_store::roles::{
    AdapterControlStore, NewAdapterDefinition, NewOrchestrationCorrelationAlias,
};
use std::sync::Arc;
use uuid::Uuid;

async fn fixture() -> (
    Arc<runinator_database::sqlite::SqliteDb>,
    AdapterDefinition,
    AdapterRevision,
    Arc<dyn Broker>,
) {
    let path = std::env::temp_dir().join(format!("adapter-control-{}.db", Uuid::now_v7()));
    let db = Arc::new(
        runinator_database::sqlite::SqliteDb::new(path.to_str().unwrap())
            .await
            .unwrap(),
    );
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let (adapter, revision) = db
        .create_orchestration_adapter(
            NewAdapterDefinition {
                id: Uuid::now_v7(),
                org_id: Uuid::now_v7(),
                name: "poll test".into(),
                kind: "github".into(),
                kind_version: "1".into(),
                transport: AdapterTransport::Polling,
                endpoint_identity: Uuid::now_v7().to_string(),
                configuration: Value::Null,
                authentication: Default::default(),
                identity_configuration: Value::Null,
                actor_id: None,
            },
            Utc::now(),
        )
        .await
        .unwrap();
    (db, adapter, revision, Arc::new(InMemoryBroker::new()))
}
fn event() -> NormalizedAdapterEvent {
    NormalizedAdapterEvent {
        source: "github".into(),
        delivery_id: "delivery-1".into(),
        event_type: "changed".into(),
        scope: "repo".into(),
        correlation_key: "pr:123".into(),
        subject_revision: Some("sha-1".into()),
        occurred_at: None,
        payload: runinator_models::json!({}),
        provenance: runinator_models::json!({"operation_key":"op-1"}),
    }
}
fn response() -> AdapterPollResponse {
    AdapterPollResponse {
        events: vec![event()],
        checkpoint: serde_json::json!({"cursor":2}),
        retry_after_seconds: None,
        error: None,
    }
}
fn request() -> AdapterPollRequest {
    AdapterPollRequest {
        configuration: serde_json::Value::Null,
        secrets: serde_json::Value::Null,
        checkpoint: serde_json::Value::Null,
        initialize: false,
    }
}

#[tokio::test]
async fn adapter_delivery_is_durable_before_checkpoint_even_when_routing_fails() {
    let (db, adapter, revision, broker) = fixture().await;
    let now = Utc::now();
    db.claim_due_orchestration_adapter_polls("claim".into(), now, now + Duration::seconds(300), 1)
        .await
        .unwrap();
    let id = adapter_polling::create_poll_attempt(
        db.clone(),
        adapter_polling::PollAttemptRequest {
            adapter: &adapter,
            revision: &revision,
            request: request(),
            claim_owner: "claim".into(),
            dry_run: false,
        },
    )
    .await
    .unwrap();
    let dispatch = db
        .fetch_orchestration_adapter_poll_dispatch(id)
        .await
        .unwrap()
        .unwrap();
    let pipelines = PipelineOperations::new(
        db.clone(),
        broker.clone(),
        UiEventPublisher::new(broker),
        None,
    );
    adapter_polling::settle_dispatched_poll(db.clone(), &pipelines, &dispatch, response())
        .await
        .unwrap();
    let mut delivery = db
        .fetch_adapter_deliveries(adapter.id, 10)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(delivery.attempt_id, Some(id));
    assert!(
        services::process_delivery(db.clone(), &pipelines, &mut delivery)
            .await
            .is_err()
    );
    assert!(
        db.fetch_adapter_delivery(delivery.id)
            .await
            .unwrap()
            .unwrap()
            .event
            .is_some()
    );
    assert_eq!(
        db.fetch_orchestration_adapter_poll_status(adapter.id)
            .await
            .unwrap()
            .unwrap()
            .checkpoint,
        runinator_models::json!({"cursor":2})
    );
    adapter_polling::settle_dispatched_poll(db.clone(), &pipelines, &dispatch, response())
        .await
        .unwrap();
    assert_eq!(
        db.fetch_adapter_deliveries(adapter.id, 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn profile_dry_run_returns_preview_without_admission_or_checkpoint_change() {
    let (db, adapter, mut revision, broker) = fixture().await;
    let profile_id = Uuid::now_v7();
    for (resource_type, resource_id) in [
        (ResourceType::OrchestrationAdapter, adapter.id),
        (ResourceType::ExecutionProfile, profile_id),
    ] {
        db.put_resource_ownership(ResourceOwnership {
            resource_type,
            resource_id,
            tenant: ScopeRef::PLATFORM,
            owner: ScopeRef::PLATFORM,
            created_by: None,
            authz_version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .await
        .unwrap();
    }
    revision.authentication = AdapterAuthentication::ExecutionProfile {
        profile: ExecutionProfileBinding::resolved(profile_id, "test"),
        required_labels: Default::default(),
        required_scopes: vec![],
    };
    let id = adapter_polling::create_poll_attempt(
        db.clone(),
        adapter_polling::PollAttemptRequest {
            adapter: &adapter,
            revision: &revision,
            request: request(),
            claim_owner: "dry-run".into(),
            dry_run: true,
        },
    )
    .await
    .unwrap();
    let dispatch = db
        .fetch_orchestration_adapter_poll_dispatch(id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dispatch.state, "queued");
    effect_consumer::settle_adapter_poll_result(
        db.clone(),
        broker.clone(),
        UiEventPublisher::new(broker),
        &dispatch,
        &EffectResultKind::Status {
            status: WorkflowEffectStatus::Succeeded,
            output: Some(serde_json::to_value(response()).unwrap().into()),
            message: None,
        },
    )
    .await
    .unwrap();
    let attempt = db
        .fetch_adapter_poll_attempts(adapter.id, 10)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(attempt.state, "succeeded");
    assert_eq!(
        attempt
            .result
            .get("previews")
            .and_then(Value::as_array)
            .unwrap()
            .len(),
        1
    );
    assert!(
        db.fetch_adapter_deliveries(adapter.id, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.fetch_orchestration_adapter_poll_status(adapter.id)
            .await
            .unwrap()
            .unwrap()
            .checkpoint
            .is_null()
    );
}

#[tokio::test]
async fn preview_resolves_alias_and_enriches_payload_like_live_delivery() {
    let (db, adapter, _, _) = fixture().await;
    let now = Utc::now();
    let admission_id = Uuid::now_v7();
    let pipeline_id = Uuid::now_v7();
    let binding_id = Uuid::now_v7();
    db.upsert_pipeline(&runinator_models::pipelines::Pipeline {
        id: Some(pipeline_id),
        name: "alias pipeline".into(),
        key: None,
        namespace: None,
        description: None,
        org_id: Some(adapter.org_id),
        enabled: true,
        graph: runinator_models::pipelines::PipelineGraph {
            version: runinator_models::pipelines::PIPELINE_GRAPH_VERSION,
            members: vec![],
            links: vec![],
            joins: Default::default(),
        },
        concurrency: Default::default(),
        defaults: Default::default(),
        metadata: Value::Null,
        created_at: None,
        updated_at: None,
    })
    .await
    .unwrap();

    db.claim_ingress_admission(
        IngressAdmission {
            id: Some(admission_id),
            org_id: Some(adapter.org_id),
            scope: "canonical".into(),
            correlation_key: "subject-1".into(),
            generation: 1,
            target: IngressTarget {
                kind: IngressTargetKind::Pipeline,
                id: pipeline_id,
            },
            status: IngressAdmissionStatus::Active,
            workflow_run_id: None,
            pipeline_run_id: None,
            policy: runinator_models::json!({"scope":"canonical","routes":[]}),
            created_at: now,
            updated_at: now,
        },
        None,
    )
    .await
    .unwrap();
    db.create_orchestration_binding(NewOrchestrationBinding {
        id: binding_id,
        admission_id,
        org_id: Some(adapter.org_id),
        scope: "canonical".into(),
        correlation_key: "subject-1".into(),
        generation: 1,
        pipeline_id,
        pipeline_revision: 1,
        pipeline_digest: "test".into(),
        adapter_id: Some(adapter.id),
        adapter_revision: Some(1),
        policy: Default::default(),
    })
    .await
    .unwrap();
    db.upsert_orchestration_correlation_alias(
        NewOrchestrationCorrelationAlias {
            id: Uuid::now_v7(),
            binding_id,
            generation: 1,
            org_id: Some(adapter.org_id),
            source: "github".into(),
            scope: "repo".into(),
            correlation_key: "pr:123".into(),
        },
        now,
    )
    .await
    .unwrap();
    let operations = AdapterOperations::new(db.clone());
    let prepared = operations
        .prepare_event(adapter.org_id, event())
        .await
        .unwrap();
    let preview = operations.preview_event(&adapter, &event()).await.unwrap();
    assert_eq!(preview["scope"], prepared.scope);
    assert_eq!(preview["correlation_key"], prepared.correlation_key);
    assert_eq!(preview["received_identity"]["correlation_key"], "pr:123");
    assert_eq!(
        prepared.payload.get("subject_revision"),
        Some(&Value::from("sha-1"))
    );
    assert!(
        prepared
            .payload
            .get("provenance")
            .unwrap()
            .get("received_correlation")
            .is_some()
    );
    assert_eq!(
        preview["existing_admission"]["id"],
        admission_id.to_string()
    );
}
