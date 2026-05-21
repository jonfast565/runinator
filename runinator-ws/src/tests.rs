use serde_json::json;

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
