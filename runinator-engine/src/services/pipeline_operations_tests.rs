use std::{path::PathBuf, sync::Arc};

use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    json,
    pipelines::{
        PIPELINE_GRAPH_VERSION, Pipeline, PipelineBundle, PipelineDefaults, PipelineGraph,
        PipelineMember, PipelineSpec, PipelineTriggerSpec,
    },
    schedules::WorkflowConcurrency,
    semver::SemVer,
    types::RuninatorType,
    value::Value,
    workflows::{WorkflowDefinition, WorkflowGraph, WorkflowTriggerKind},
};
use runinator_store::{
    DatabaseImpl,
    roles::{DefinitionStore, ScheduleStore},
};

use super::*;

async fn test_db() -> (Arc<SqliteDb>, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-pipeline-operations-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (Arc::new(db), path)
}

fn member_workflow() -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: "pipeline member".into(),
        key: Some("pipeline_member".into()),
        namespace: Some("runinator.tests".into()),
        org_id: None,
        version: SemVer::new(1, 0, 0),
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    }
}

fn pipeline() -> Pipeline {
    Pipeline {
        id: None,
        name: "service boundary".into(),
        key: Some("service_boundary".into()),
        namespace: Some("runinator.tests".into()),
        description: None,
        org_id: None,
        graph: PipelineGraph {
            version: PIPELINE_GRAPH_VERSION,
            ..Default::default()
        },
        concurrency: WorkflowConcurrency::default(),
        defaults: PipelineDefaults::default(),
        metadata: Value::default(),
        created_at: None,
        updated_at: None,
    }
}

#[tokio::test]
async fn save_persists_a_valid_pipeline_through_the_service() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = PipelineOperations::new(
        db.clone(),
        broker.clone(),
        UiEventPublisher::new(broker),
        None,
    );

    let saved = service.save(&pipeline()).await.unwrap();
    let id = saved.id.expect("saved pipeline receives an id");
    assert_eq!(
        service.fetch(id).await.unwrap().unwrap().name,
        "service boundary"
    );
    let revisions = db.fetch_pipeline_revisions(id, 10).await.unwrap();
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].revision, 1);
    assert!(revisions[0].digest.starts_with("sha256:"));

    // Re-saving an identical head is idempotent; an executable edit creates the next snapshot.
    service.save(&saved).await.unwrap();
    assert_eq!(db.fetch_pipeline_revisions(id, 10).await.unwrap().len(), 1);
    let mut edited = saved;
    edited.metadata = runinator_models::json!({ "release": 2 });
    service.save(&edited).await.unwrap();
    let revisions = db.fetch_pipeline_revisions(id, 10).await.unwrap();
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].revision, 2);
    assert_ne!(revisions[0].digest, revisions[1].digest);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn save_requires_pipeline_identity_and_canonical_member_keys() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = PipelineOperations::new(db, broker.clone(), UiEventPublisher::new(broker), None);

    let mut missing_namespace = pipeline();
    missing_namespace.namespace = None;
    let error = service.save(&missing_namespace).await.unwrap_err();
    assert!(error.to_string().contains("namespace is required"));

    let mut missing_key = pipeline();
    missing_key.key = None;
    let error = service.save(&missing_key).await.unwrap_err();
    assert!(error.to_string().contains("key is required"));

    let mut bare_member = pipeline();
    bare_member.graph.members.push(PipelineMember {
        key: "display name".into(),
        workflow_id: uuid::Uuid::now_v7(),
        failure_mode: Default::default(),
    });
    let error = service.save(&bare_member).await.unwrap_err();
    assert!(error.to_string().contains("canonical namespace.key"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn save_validates_ingress_and_orchestration_as_one_pipeline_contract() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = PipelineOperations::new(db, broker.clone(), UiEventPublisher::new(broker), None);

    let mut unmanaged_dispatch = pipeline();
    unmanaged_dispatch.metadata = json!({
        "ingress": {
            "scope": "items",
            "routes": [{
                "event_type": "updated",
                "lifecycle": "active",
                "action": "dispatch",
                "intent": "refresh"
            }]
        }
    });
    let error = service.save(&unmanaged_dispatch).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires an orchestration policy")
    );

    let mut missing_ingress = pipeline();
    missing_ingress.metadata = json!({
        "orchestration": {
            "intents": {
                "refresh": { "effect": "observe", "priority": 10 }
            }
        }
    });
    let error = service.save(&missing_ingress).await.unwrap_err();
    assert!(error.to_string().contains("requires an ingress policy"));

    let mut unknown_intent = pipeline();
    unknown_intent.metadata = json!({
        "ingress": {
            "scope": "items",
            "routes": [{
                "event_type": "created",
                "lifecycle": "unbound",
                "action": "start"
            }, {
                "event_type": "updated",
                "lifecycle": "active",
                "action": "dispatch",
                "intent": "missing"
            }]
        },
        "orchestration": {
            "intents": {
                "refresh": { "effect": "observe", "priority": 10 }
            }
        }
    });
    let error = service.save(&unknown_intent).await.unwrap_err();
    assert!(error.to_string().contains("does not exist"));

    unknown_intent
        .metadata
        .pointer_mut("/ingress/routes/1/intent")
        .map(|intent| *intent = Value::String("refresh".into()));
    service
        .save(&unknown_intent)
        .await
        .expect("matching ingress and orchestration policies save together");

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_uses_canonical_members_and_resolves_same_pack_pipeline_sources() {
    let (db, path) = test_db().await;
    let mut alpha = member_workflow();
    alpha.name = "Build".into();
    alpha.key = Some("build".into());
    alpha.namespace = Some("acme.alpha".into());
    db.upsert_workflow(&alpha).await.unwrap();

    let mut beta = member_workflow();
    beta.name = "Build".into();
    beta.key = Some("build".into());
    beta.namespace = Some("acme.beta".into());
    let beta = db.upsert_workflow(&beta).await.unwrap();
    let beta_id = beta.id.unwrap();

    let source = PipelineSpec {
        name: "Source".into(),
        key: Some("source".into()),
        namespace: Some("acme.pipelines".into()),
        description: None,
        defaults: Default::default(),
        members: vec!["acme.beta.build".into()],
        links: Vec::new(),
        joins: Vec::new(),
        concurrency: Default::default(),
        metadata: runinator_models::json!({
            "ingress": {
                "scope": "acme.release",
                "routes": [{
                    "event_type": "created",
                    "lifecycle": "unbound",
                    "action": "start"
                }]
            }
        }),
        triggers: Vec::new(),
    };
    let target = PipelineSpec {
        name: "Target".into(),
        key: Some("target".into()),
        namespace: Some("acme.pipelines".into()),
        description: None,
        defaults: Default::default(),
        members: vec!["acme.beta.build".into()],
        links: Vec::new(),
        joins: Vec::new(),
        concurrency: Default::default(),
        metadata: runinator_models::json!({}),
        triggers: vec![
            PipelineTriggerSpec {
                kind: WorkflowTriggerKind::Chained,
                enabled: true,
                configuration: json!({
                    "on": "success",
                    "source_pipeline": "acme.pipelines.source",
                    "parameters": {},
                }),
            },
            PipelineTriggerSpec {
                kind: WorkflowTriggerKind::Chained,
                enabled: true,
                configuration: json!({
                    "on": "complete",
                    "source_workflow": "acme.beta.build",
                    "parameters": {},
                }),
            },
        ],
    };

    let imported = crate::repository::import_pipeline_bundle_with(
        db.as_ref(),
        &PipelineBundle {
            pipelines: vec![source, target],
        },
        None,
    )
    .await
    .unwrap();
    let source_id = imported[0].id.unwrap();
    let target_id = imported[1].id.unwrap();
    assert_eq!(imported[1].graph.members[0].workflow_id, beta_id);
    assert_eq!(
        imported[0]
            .metadata
            .get("ingress")
            .and_then(|value| value.get("scope"))
            .and_then(Value::as_str),
        Some("acme.release")
    );
    assert_eq!(
        imported[0]
            .metadata
            .get("managed_by")
            .and_then(Value::as_str),
        Some("rexrap")
    );

    let triggers = db.fetch_pipeline_triggers(target_id).await.unwrap();
    assert_eq!(triggers.len(), 2);
    let pipeline_trigger = triggers
        .iter()
        .find(|trigger| trigger.configuration.get("source_pipeline").is_some())
        .unwrap();
    assert_eq!(
        pipeline_trigger
            .configuration
            .get("source_pipeline")
            .and_then(Value::as_str),
        Some("acme.pipelines.source")
    );
    assert_eq!(
        pipeline_trigger
            .configuration
            .get("source_pipeline_id")
            .and_then(Value::as_str),
        Some(source_id.to_string().as_str())
    );
    let workflow_trigger = triggers
        .iter()
        .find(|trigger| trigger.configuration.get("source_workflow").is_some())
        .unwrap();
    assert_eq!(
        workflow_trigger
            .configuration
            .get("source_workflow_id")
            .and_then(Value::as_str),
        Some(beta_id.to_string().as_str())
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_pinned_pipeline_start_snapshots_the_requested_revision() {
    let (db, path) = test_db().await;
    let workflow = db.upsert_workflow(&member_workflow()).await.unwrap();
    let workflow_id = workflow.id.unwrap();
    let broker = Arc::new(InMemoryBroker::new());
    let service = PipelineOperations::new(db, broker.clone(), UiEventPublisher::new(broker), None);
    let mut first = pipeline();
    first.graph.members.push(PipelineMember {
        key: "runinator.tests.pipeline_member".into(),
        workflow_id,
        failure_mode: Default::default(),
    });
    first.metadata = json!({ "release": 1 });
    let first = service.save(&first).await.unwrap();
    let pipeline_id = first.id.unwrap();
    let mut second = first;
    second.metadata = json!({ "release": 2 });
    service.save(&second).await.unwrap();

    let run = service
        .create_run(
            pipeline_id,
            Value::Object(Default::default()),
            Some(1),
            Some("test".into()),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        run.pipeline_snapshot
            .as_ref()
            .and_then(|pipeline| pipeline.metadata.get("release"))
            .and_then(Value::as_i64),
        Some(1)
    );

    let _ = std::fs::remove_file(path);
}
