use std::{path::PathBuf, sync::Arc, time::Duration};

use runinator_broker_core::{EmbeddedEngineSignals, UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    json,
    replicas::WorkflowRunProvenance,
    revisions::{RevisionAuthor, RevisionSource},
    semver::SemVer,
    types::RuninatorType,
    workflows::{WorkflowDefinition, WorkflowGraph},
};
use runinator_store::{DatabaseImpl, RuntimeStore, roles::WorkflowVmStore};

use super::*;
use crate::repository;

async fn test_db() -> (Arc<SqliteDb>, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-run-operations-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (Arc::new(db), path)
}

fn workflow() -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: "run operations".into(),
        key: None,
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

#[tokio::test]
async fn create_persists_and_nudges_the_embedded_engine() {
    let (db, path) = test_db().await;
    let saved = repository::upsert_workflow(
        db.as_ref(),
        &workflow(),
        &RevisionAuthor::system(RevisionSource::Api),
    )
    .await
    .unwrap();
    let workflow_id = saved.id.expect("saved workflow receives an id");
    let broker = Arc::new(InMemoryBroker::new());
    let signals = EmbeddedEngineSignals::new();
    let service = RunOperations::new(
        db.clone(),
        broker.clone(),
        UiEventPublisher::new(broker.clone()),
        Some(signals.clone()),
    );

    let run = service
        .create(
            workflow_id,
            json!({ "ticket": "R-42" }),
            false,
            None,
            WorkflowRunProvenance::default(),
        )
        .await
        .unwrap();
    assert!(db.fetch_workflow_run(run.id).await.unwrap().is_some());
    let continuations = db.fetch_workflow_continuations(run.id).await.unwrap();
    assert_eq!(continuations.len(), 1);
    assert_eq!(continuations[0].locals.get("config"), Some(&json!({})));

    tokio::time::timeout(
        Duration::from_secs(1),
        signals.workflow_vm_notifier().notified(),
    )
    .await
    .expect("embedded engine is nudged after the durable write");

    let _ = std::fs::remove_file(path);
}
