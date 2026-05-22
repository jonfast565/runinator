use super::*;
use crate::interfaces::DatabaseImpl;

#[tokio::test]
async fn fetch_recent_workflow_runs_returns_all_workflows_newest_first() {
    let path = std::env::temp_dir().join(format!(
        "runinator-workflow-runs-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let first = db
        .upsert_workflow(&workflow("first"))
        .await
        .unwrap()
        .id
        .unwrap();
    let second = db
        .upsert_workflow(&workflow("second"))
        .await
        .unwrap()
        .id
        .unwrap();
    let first_snapshot = db.fetch_workflow(first).await.unwrap().unwrap();
    let second_snapshot = db.fetch_workflow(second).await.unwrap().unwrap();
    let older = db
        .create_workflow_run(
            first,
            first_snapshot,
            serde_json::json!({}),
            serde_json::json!({}),
        )
        .await
        .unwrap();
    let newer = db
        .create_workflow_run(
            second,
            second_snapshot,
            serde_json::json!({}),
            serde_json::json!({}),
        )
        .await
        .unwrap();

    let runs = db.fetch_recent_workflow_runs().await.unwrap();
    assert_eq!(
        runs.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![newer.id, older.id]
    );
    assert_eq!(
        runs.iter().map(|run| run.workflow_id).collect::<Vec<_>>(),
        vec![second, first]
    );
    assert_eq!(
        runs[0]
            .workflow_snapshot
            .as_ref()
            .map(|workflow| workflow.name.as_str()),
        Some("second")
    );

    let _ = fs::remove_file(path);
}

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        version: 1,
        enabled: true,
        input_schema: serde_json::json!({}),
        definition: serde_json::json!({ "nodes": [] }),
        created_at: None,
        updated_at: None,
    }
}
