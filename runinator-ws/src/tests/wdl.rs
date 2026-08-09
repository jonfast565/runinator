//! the author-time endpoints: wdl evaluate/analyze over source fragments, and the node/trigger/enum
//! catalogs the ui builds its palette from.

use super::*;

#[tokio::test]
async fn wdl_evaluate_accepts_legacy_lowered_expression() {
    let request = crate::handlers::wdl::EvaluateExpressionRequest {
        expression: Some(json!({ "$concat": ["hello ", { "$ref": { "params": ["name"] } }] })),
        source: None,
        kind: WdlFragmentKind::Expression,
        context: json!({ "input": { "name": "Ada" } }),
    };

    let Json(value) = crate::handlers::wdl::evaluate_expression(Json(request))
        .await
        .expect("evaluate");

    assert_eq!(value, Value::from("hello Ada"));
}

#[tokio::test]
async fn wdl_evaluate_accepts_source_fragments() {
    let request = crate::handlers::wdl::EvaluateExpressionRequest {
        expression: None,
        source: Some("params.count >= 3 && exists params.count".into()),
        kind: WdlFragmentKind::Condition,
        context: json!({ "input": { "count": 3 } }),
    };

    let Json(value) = crate::handlers::wdl::evaluate_expression(Json(request))
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
    // assert the names rather than a bare count: the ui looks each one up by name, so a rename is
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
async fn wdl_analyze_validates_source_fragments() {
    let (db, path) = test_db().await;
    let Json(diagnostics) = crate::handlers::wdl::analyze_wdl(
        Extension(Arc::new(db)),
        Json(crate::handlers::wdl::WdlSourceRequest {
            source: "params.count >".into(),
            fragment: Some(WdlFragmentKind::Condition),
        }),
    )
    .await;

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, "error");
    let _ = std::fs::remove_file(path);
}
