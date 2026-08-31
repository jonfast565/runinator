//! the author-time endpoints: rexrap evaluate/analyze over source fragments, and the node/trigger/enum
//! catalogs the UI builds its palette from.

use super::*;

use runinator_engine::services::{CatalogOperations, WorkflowAuthoring};

#[tokio::test]
async fn rexrap_evaluate_accepts_legacy_lowered_expression() {
    let request = crate::handlers::rexrap::EvaluateExpressionRequest {
        expression: Some(json!({ "$concat": ["hello ", { "$ref": { "params": ["name"] } }] })),
        source: None,
        kind: RexRapFragmentKind::Expression,
        context: json!({ "input": { "name": "Ada" } }),
    };

    let Json(value) = crate::handlers::rexrap::evaluate_expression(ValidatedJson(request))
        .await
        .expect("evaluate");

    assert_eq!(value, Value::from("hello Ada"));
}

#[tokio::test]
async fn rexrap_evaluate_accepts_source_fragments() {
    let request = crate::handlers::rexrap::EvaluateExpressionRequest {
        expression: None,
        source: Some("params.count >= 3 && exists params.count".into()),
        kind: RexRapFragmentKind::Condition,
        context: json!({ "input": { "count": 3 } }),
    };

    let Json(value) = crate::handlers::rexrap::evaluate_expression(ValidatedJson(request))
        .await
        .expect("evaluate");

    assert_eq!(value, Value::from(true));
}

#[tokio::test]
async fn get_node_kinds_returns_catalog_json() {
    let (status, Json(response)) = crate::handlers::catalog_metadata::get_node_kinds().await;

    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::JsonValue(value) = response else {
        panic!("node catalog response must be json");
    };
    assert_eq!(
        value.as_array().map(Vec::len),
        Some(runinator_models::workflows::WorkflowNodeKind::ALL.len()),
        "the catalog serves every node kind; a new kind must reach the ui palette"
    );
}

#[tokio::test]
async fn get_trigger_kinds_returns_catalog_json() {
    let (status, Json(response)) = crate::handlers::catalog_metadata::get_trigger_kinds().await;

    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::JsonValue(value) = response else {
        panic!("trigger catalog response must be json");
    };
    // cron, manual, chained.
    assert_eq!(value.as_array().map(Vec::len), Some(3));
}

#[tokio::test]
async fn get_enum_catalogs_returns_catalog_json() {
    let (status, Json(response)) = crate::handlers::catalog_metadata::get_enum_catalogs().await;

    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::JsonValue(value) = response else {
        panic!("enum catalog response must be json");
    };
    // Assert the names rather than a count. The UI looks each one up by name, so a rename is
    // the failure that matters and a count alone would not say which entry moved.
    let names: Vec<&str> = value
        .as_array()
        .expect("enum catalog must be an array")
        .iter()
        .filter_map(|entry| entry.get("name").and_then(|name| name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            "gate_kind",
            "match_kind",
            "branch_policy",
            "setting_kind",
            "interrupt_source",
            "resume_mode",
            "concurrency_policy",
        ]
    );
}

#[tokio::test]
async fn rexrap_analyze_validates_source_fragments() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let (tx, _rx) = tokio::sync::broadcast::channel(64);
    let events = crate::events::EventBus::new(tx, Arc::new(InMemoryBroker::new()));
    let Json(diagnostics) = crate::handlers::rexrap::analyze_rexrap(
        Extension(Arc::new(CatalogOperations::new(db.clone()))),
        Extension(Arc::new(WorkflowAuthoring::new(db, events.publisher()))),
        ValidatedJson(crate::handlers::rexrap::RexRapSourceRequest {
            source: "params.count >".into(),
            fragment: Some(RexRapFragmentKind::Condition),
        }),
    )
    .await;

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, "error");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn rexrap_save_mints_new_ids_and_preserves_the_active_org() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let (tx, _rx) = tokio::sync::broadcast::channel(64);
    let events = crate::events::EventBus::new(tx, Arc::new(InMemoryBroker::new()));
    let catalog = Arc::new(CatalogOperations::new(db.clone()));
    let authoring = Arc::new(WorkflowAuthoring::new(db.clone(), events.publisher()));
    let org_id = Uuid::now_v7();
    let mut ctx = AuthContext::disabled_platform_admin();
    ctx.org_id = Some(org_id);
    let source = r#"
        namespace acme.delivery {
            workflow "Release workflow" v1 {
                key release_train
                do { return }
            }
        }
    "#;

    let (status, Json(response)) = crate::handlers::rexrap::import_rexrap(
        Extension(db.clone()),
        Extension(catalog.clone()),
        Extension(authoring.clone()),
        Extension(ctx.clone()),
        ValidatedJson(crate::handlers::rexrap::ImportRexRapRequest {
            source: source.into(),
            enabled: true,
            workflow_id: None,
            triggers: Vec::new(),
            ui: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::WorkflowBundle(created) = response else {
        panic!("expected saved workflow bundle");
    };
    let created = created.workflows.into_iter().next().unwrap();
    let workflow_id = created.id.expect("create mints an id");
    assert_eq!(created.org_id, Some(org_id));

    let renamed_source = source.replace("Release workflow", "Release workflow renamed");
    let (status, Json(response)) = crate::handlers::rexrap::import_rexrap(
        Extension(db.clone()),
        Extension(catalog.clone()),
        Extension(authoring.clone()),
        Extension(ctx.clone()),
        ValidatedJson(crate::handlers::rexrap::ImportRexRapRequest {
            source: renamed_source,
            enabled: true,
            workflow_id: Some(workflow_id),
            triggers: Vec::new(),
            ui: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::WorkflowBundle(updated) = response else {
        panic!("expected updated workflow bundle");
    };
    assert_eq!(updated.workflows[0].org_id, Some(org_id));

    let (status, _) = crate::handlers::rexrap::import_rexrap(
        Extension(db.clone()),
        Extension(catalog),
        Extension(authoring),
        Extension(ctx),
        ValidatedJson(crate::handlers::rexrap::ImportRexRapRequest {
            source: source.into(),
            enabled: true,
            workflow_id: None,
            triggers: Vec::new(),
            ui: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(db.fetch_workflows().await.unwrap().len(), 1);

    let _ = std::fs::remove_file(path);
}
