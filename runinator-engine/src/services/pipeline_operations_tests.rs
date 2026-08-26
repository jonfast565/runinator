use std::{path::PathBuf, sync::Arc};

use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    json,
    pipelines::{
        PIPELINE_GRAPH_VERSION, Pipeline, PipelineDefaults, PipelineGraph, PipelineMember,
    },
    schedules::WorkflowConcurrency,
    semver::SemVer,
    types::RuninatorType,
    value::Value,
    workflows::{WorkflowDefinition, WorkflowGraph},
};
use runinator_store::{DatabaseImpl, roles::DefinitionStore};

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
        namespace: None,
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
        key: None,
        namespace: None,
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
async fn a_pinned_pipeline_start_snapshots_the_requested_revision() {
    let (db, path) = test_db().await;
    let workflow = db.upsert_workflow(&member_workflow()).await.unwrap();
    let workflow_id = workflow.id.unwrap();
    let broker = Arc::new(InMemoryBroker::new());
    let service = PipelineOperations::new(db, broker.clone(), UiEventPublisher::new(broker), None);
    let mut first = pipeline();
    first.graph.members.push(PipelineMember {
        key: "member".into(),
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
