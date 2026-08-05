//! validating a definition without persisting it, and the structured diagnostics a rejection
//! carries back to the editor.

use super::*;

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
    assert_eq!(validated.definition.start.as_deref(), Some("start"));
}

#[test]
fn validate_workflow_rejects_invalid_definition_without_persistence() {
    let mut workflow = workflow(None, "invalid");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "missing" } } },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();

    assert!(crate::repository::validate_workflow_definition(&workflow).is_err());
}

#[test]
fn validation_error_response_exposes_structured_type_diagnostic() {
    let err = WorkflowValidationError::TypeDiagnostic(WorkflowTypeDiagnostic {
        path: "action parameter 'config.name'".into(),
        expected: "string".into(),
        actual: "integer".into(),
        message: "action parameter 'config.name' expected string, got integer".into(),
    });

    let (status, axum::Json(response)) = crate::responses::validation_error(&err);
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    match response {
        crate::models::ApiResponse::ApiError(error) => {
            assert_eq!(
                error.path.as_deref(),
                Some("action parameter 'config.name'")
            );
            assert_eq!(error.expected.as_deref(), Some("string"));
            assert_eq!(error.actual.as_deref(), Some("integer"));
        }
        _ => panic!("expected api error"),
    }
}

#[tokio::test]
async fn validate_workflow_rejects_invalid_subflow_id() {
    let (db, path) = test_db().await;

    // create a valid target workflow
    let target = crate::repository::upsert_workflow(&db, &workflow(None, "target-workflow"))
        .await
        .unwrap();
    let target_id = target.id.unwrap();

    // create a workflow with a subflow that references a non-existent workflow
    let mut main_workflow = workflow(None, "main-with-invalid-subflow");
    main_workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "subflow-node" } } },
            {
                "id": "subflow-node",
                "kind": "subflow",
                "subflow_id": Uuid::now_v7().to_string(),  // non-existent workflow id
                "transitions": { "next": { "$node": "end" } }
            },
            { "id": "end", "kind": "end" }
        ]
    }))
    .unwrap();

    // validation should fail because the subflow references a non-existent workflow
    let result =
        crate::repository::validate_workflow_definition_with_catalog(&db, &main_workflow).await;
    assert!(result.is_err());

    // now test with a valid subflow id
    let mut valid_workflow = workflow(None, "main-with-valid-subflow");
    valid_workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "subflow-node" } } },
            {
                "id": "subflow-node",
                "kind": "subflow",
                "subflow_id": target_id,
                "transitions": { "next": { "$node": "end" } }
            },
            { "id": "end", "kind": "end" }
        ]
    }))
    .unwrap();

    // validation should succeed because the subflow references a valid workflow
    let result =
        crate::repository::validate_workflow_definition_with_catalog(&db, &valid_workflow).await;
    assert!(result.is_ok());

    let _ = std::fs::remove_file(path);
}
