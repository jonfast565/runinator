use std::{path::PathBuf, sync::Arc};

use runinator_broker_core::{UiEventPublisher, in_memory::InMemoryBroker};
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    pipelines::{PIPELINE_GRAPH_VERSION, Pipeline, PipelineDefaults, PipelineGraph},
    schedules::WorkflowConcurrency,
    value::Value,
};
use runinator_store::DatabaseImpl;

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

fn pipeline() -> Pipeline {
    Pipeline {
        id: None,
        name: "service boundary".into(),
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

    let _ = std::fs::remove_file(path);
}
