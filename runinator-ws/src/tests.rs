use serde_json::json;

use runinator_database::{interfaces::DatabaseImpl, sqlite::SqliteDb};
use runinator_models::workflows::{
    WorkflowBundle, WorkflowDefinition, WorkflowStatus, WorkflowTrigger, WorkflowTriggerKind,
};

#[test]
fn workflow_run_stream_terminal_status_stays_snapshot_message() {
    let response = crate::models::WorkflowRunResponse {
        run: runinator_models::workflows::WorkflowRun {
            id: 42,
            workflow_id: 7,
            workflow_snapshot: None,
            status: runinator_models::workflows::WorkflowStatus::Succeeded,
            active_node_id: None,
            parameters: json!({}),
            state: json!({}),
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: Some(chrono::Utc::now()),
            message: None,
            name: None,
        },
        nodes: vec![],
    };

    let value = serde_json::to_value(response).unwrap();

    assert_eq!(value["run"]["status"], "succeeded");
    assert_eq!(value["nodes"], json!([]));
    assert!(value.get("type").is_none());
}

#[test]
fn workflow_run_request_defaults_to_non_debug() {
    let request: crate::models::WorkflowRunRequest =
        serde_json::from_value(json!({ "parameters": { "mode": "test" } })).unwrap();

    assert!(!request.debug);
    assert_eq!(request.parameters["mode"], "test");
}

#[test]
fn workflow_run_request_accepts_debug_flag() {
    let request: crate::models::WorkflowRunRequest =
        serde_json::from_value(json!({ "parameters": {}, "debug": true })).unwrap();

    assert!(request.debug);
}

#[tokio::test]
async fn workflow_runs_can_be_named_and_fetched_by_open_name() {
    let (db, path) = test_db().await;
    let workflow = crate::repository::upsert_workflow(&db, &workflow(None, "Ticket Work"))
        .await
        .unwrap();
    let workflow_id = workflow.id.unwrap();
    let open = crate::repository::create_workflow_run(
        &db,
        workflow_id,
        json!({}),
        false,
        Some("Ticket Work: ITP-123".into()),
    )
    .await
    .unwrap();
    let terminal = crate::repository::create_workflow_run(
        &db,
        workflow_id,
        json!({}),
        false,
        Some("Ticket Work: ITP-123".into()),
    )
    .await
    .unwrap();
    crate::repository::update_workflow_run_status(
        &db,
        terminal.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let open_only =
        crate::repository::fetch_workflow_runs_by_name(&db, "Ticket Work: ITP-123".into(), true)
            .await
            .unwrap();

    assert_eq!(open.name.as_deref(), Some("Ticket Work: ITP-123"));
    assert_eq!(
        open_only.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![open.id]
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn merges_json_objects() {
    let defaults = json!({ "a": 1, "b": 2 });
    let parameters = json!({ "b": 3, "c": 4 });
    let merged = crate::repository::merge_json_object(&defaults, &parameters);
    assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
}

#[test]
fn registered_provider_items_become_provider_metadata() {
    let providers = crate::provider_metadata_from_items(vec![json!({
        "document": {
            "name": "github",
            "actions": [
                { "function_name": "create_pr", "description": "Create a pull request" }
            ]
        }
    })])
    .expect("provider metadata parses");

    assert_eq!(providers[0].name, "github");
    assert_eq!(providers[0].actions[0].function_name, "create_pr");
}

#[test]
fn provider_metadata_becomes_registered_catalog_item() {
    let item = crate::provider_catalog_item(&runinator_models::providers::ProviderMetadata {
        name: "git".into(),
        actions: vec![runinator_models::providers::ActionMetadata::new(
            "diff", "Get diff",
        )],
        metadata: Default::default(),
    });

    assert_eq!(item["item_type"], "provider_metadata");
    assert_eq!(item["document"]["name"], "git");
    assert_eq!(item["document"]["actions"][0]["function_name"], "diff");
}

#[test]
fn validate_workflow_returns_normalized_definition() {
    let workflow = workflow(None, "validate");
    let validated = crate::repository::validate_workflow_definition(&workflow).unwrap();

    assert_eq!(validated.name, "validate");
    assert_eq!(validated.definition["start"], "start");
}

#[test]
fn validate_workflow_rejects_invalid_definition_without_persistence() {
    let mut workflow = workflow(None, "invalid");
    workflow.definition = json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "missing" } } },
            { "id": "done", "kind": "end" }
        ]
    });

    assert!(crate::repository::validate_workflow_definition(&workflow).is_err());
}

#[tokio::test]
async fn export_all_includes_workflows_and_matching_triggers() {
    let (db, path) = test_db().await;
    let saved = crate::repository::upsert_workflow(&db, &workflow(None, "export-all"))
        .await
        .unwrap();
    let workflow_id = saved.id.unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, workflow_id))
        .await
        .unwrap();

    let bundle = crate::repository::export_workflow_bundle(&db, None)
        .await
        .unwrap();

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].id, Some(workflow_id));
    assert_eq!(bundle.triggers.len(), 1);
    assert_eq!(bundle.triggers[0].workflow_id, workflow_id);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn export_one_includes_only_that_workflow_and_its_triggers() {
    let (db, path) = test_db().await;
    let first = crate::repository::upsert_workflow(&db, &workflow(None, "first"))
        .await
        .unwrap();
    let second = crate::repository::upsert_workflow(&db, &workflow(None, "second"))
        .await
        .unwrap();
    let first_id = first.id.unwrap();
    let second_id = second.id.unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, first_id))
        .await
        .unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, second_id))
        .await
        .unwrap();

    let bundle = crate::repository::export_workflow_bundle(&db, Some(second_id))
        .await
        .unwrap();

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].id, Some(second_id));
    assert_eq!(bundle.triggers.len(), 1);
    assert_eq!(bundle.triggers[0].workflow_id, second_id);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_upserts_workflows_before_triggers() {
    let (db, path) = test_db().await;
    let bundle = WorkflowBundle {
        workflows: vec![workflow(Some(77), "imported")],
        triggers: vec![trigger(Some(88), 77)],
    };

    let saved = crate::repository::import_workflow_bundle(&db, bundle)
        .await
        .unwrap();

    assert_eq!(saved.workflows[0].id, Some(77));
    assert_eq!(saved.triggers[0].id, Some(88));
    assert!(db.fetch_workflow(77).await.unwrap().is_some());
    assert_eq!(db.fetch_workflow_triggers(77).await.unwrap().len(), 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_reuses_existing_workflow_by_name_when_id_is_missing() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
        triggers: vec![],
    };
    crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let mut changed = workflow(None, "Core Team SDLC Pipeline");
    changed.version = 2;
    changed.definition = json!({
        "start": "done",
        "nodes": [
            { "id": "done", "kind": "end" }
        ]
    });
    let second = WorkflowBundle {
        workflows: vec![changed.clone()],
        triggers: vec![],
    };

    let saved = crate::repository::import_workflow_bundle(&db, second)
        .await
        .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(saved.workflows[0].id, workflows[0].id);
    assert_eq!(workflows[0].name, "Core Team SDLC Pipeline");
    assert_eq!(workflows[0].version, 2);
    let _ = std::fs::remove_file(path);
}

async fn test_db() -> (SqliteDb, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-ws-workflows-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (db, path)
}

fn workflow(id: Option<i64>, name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id,
        name: name.into(),
        version: 1,
        enabled: true,
        input_schema: json!({ "type": "object" }),
        definition: json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
                { "id": "done", "kind": "end" }
            ]
        }),
        created_at: None,
        updated_at: None,
    }
}

#[test]
fn ancestors_in_snapshot_returns_topological_path() {
    let snapshot = WorkflowDefinition {
        id: Some(1),
        name: "ancestors".into(),
        version: 1,
        enabled: true,
        input_schema: json!({}),
        definition: json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "a" } } },
                { "id": "a", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "b" } } },
                { "id": "b", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "c" } } },
                { "id": "c", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }),
        created_at: None,
        updated_at: None,
    };
    let ancestors = crate::repository::ancestors_in_snapshot(&snapshot, "c").unwrap();
    assert!(ancestors.contains(&"start".to_string()));
    assert!(ancestors.contains(&"a".to_string()));
    assert!(ancestors.contains(&"b".to_string()));
    assert!(!ancestors.contains(&"c".to_string()));
    // start must come before a, a before b.
    let pos_start = ancestors.iter().position(|n| n == "start").unwrap();
    let pos_a = ancestors.iter().position(|n| n == "a").unwrap();
    let pos_b = ancestors.iter().position(|n| n == "b").unwrap();
    assert!(pos_start < pos_a);
    assert!(pos_a < pos_b);
}

#[test]
fn ancestors_in_snapshot_refuses_control_flow_ancestor() {
    let snapshot = WorkflowDefinition {
        id: Some(1),
        name: "loop_ancestor".into(),
        version: 1,
        enabled: true,
        input_schema: json!({}),
        definition: json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "loop1" } } },
                { "id": "loop1", "kind": "loop", "parameters": { "items": [], "target": { "$node": "inside" } }, "transitions": { "next": { "$node": "end" } } },
                { "id": "inside", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "loop1" } } },
                { "id": "end", "kind": "end" }
            ]
        }),
        created_at: None,
        updated_at: None,
    };
    let result = crate::repository::ancestors_in_snapshot(&snapshot, "inside");
    assert!(
        result.is_err(),
        "expected refusal for control-flow ancestor"
    );
    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("control-flow") || message.contains("Loop") || message.contains("safely"),
        "error should mention control flow: {message}"
    );
}

#[test]
fn ancestors_in_snapshot_rejects_missing_step() {
    let snapshot = WorkflowDefinition {
        id: Some(1),
        name: "missing".into(),
        version: 1,
        enabled: true,
        input_schema: json!({}),
        definition: json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }),
        created_at: None,
        updated_at: None,
    };
    let result = crate::repository::ancestors_in_snapshot(&snapshot, "nope");
    assert!(result.is_err());
}

fn trigger(id: Option<i64>, workflow_id: i64) -> WorkflowTrigger {
    WorkflowTrigger {
        id,
        workflow_id,
        kind: WorkflowTriggerKind::Manual,
        enabled: true,
        configuration: json!({}),
        next_execution: None,
        blackout_start: None,
        blackout_end: None,
        metadata: json!({}),
        created_at: None,
        updated_at: None,
    }
}
