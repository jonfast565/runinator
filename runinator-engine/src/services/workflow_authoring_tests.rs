use std::{path::PathBuf, sync::Arc};

use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    json,
    revisions::{RevisionAuthor, RevisionSource},
    semver::SemVer,
    types::RuninatorType,
    workflows::{WorkflowDefinition, WorkflowGraph},
};
use runinator_store::{DatabaseImpl, RuntimeStore};

use super::*;

async fn test_db() -> (Arc<SqliteDb>, PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-authoring-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (Arc::new(db), path)
}

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.into(),
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
async fn save_persists_an_authored_workflow() {
    let (db, path) = test_db().await;
    let broker = Arc::new(InMemoryBroker::new());
    let service = WorkflowAuthoring::new(db.clone(), UiEventPublisher::new(broker.clone()));

    let saved = service
        .save(
            &workflow("authoring service"),
            &RevisionAuthor::system(RevisionSource::Api),
        )
        .await
        .unwrap();
    let workflow_id = saved.id.expect("saved workflow receives an id");
    assert!(db.fetch_workflow(workflow_id).await.unwrap().is_some());

    let _ = std::fs::remove_file(path);
}
