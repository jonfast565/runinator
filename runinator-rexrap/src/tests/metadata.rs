//! portable workflow metadata and editor presentation state.

use super::*;

#[test]
fn model_originated_metadata_and_ui_round_trip_through_source() {
    let mut workflow = compile(r#"workflow "Portable" v1 { do { console.run(command: "ok") } }"#);
    workflow.definition.metadata = runinator_models::json!({
        "owner_note": "keep me",
        "labels": { "tier": "control" },
    });
    workflow.definition.extra.insert(
        "ui".into(),
        runinator_models::json!({
            "layout": { "nodes": { "start": { "x": 10, "y": 20 } } }
        }),
    );

    let source = decompile(&workflow).expect("decompile portable workflow state");
    assert!(source.contains("metadata"), "{source}");
    assert!(source.contains("owner_note"), "{source}");
    assert!(source.contains("ui"), "{source}");
    let reparsed = compile(&source);
    assert_eq!(
        reparsed.definition.metadata.get("owner_note"),
        Some(&Value::from("keep me"))
    );
    assert_eq!(
        reparsed.definition.extra.get("ui"),
        workflow.definition.extra.get("ui")
    );
}

#[test]
fn source_rejects_reserved_workflow_metadata() {
    let error = compile_str_strict(
        r#"
namespace runinator.tests
workflow "Reserved" v1 {
    key reserved
    metadata { managed_by: "functions" }
    do { console.run(command: "ok") }
}
"#,
        &CompileOptions::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("managed_by"), "{error}");
}

#[test]
fn decompile_rejects_unrepresentable_graph_sidecars() {
    let mut workflow = compile(r#"workflow "Portable" v1 { do { console.run(command: "ok") } }"#);
    workflow
        .definition
        .extra
        .insert("hidden".into(), Value::Bool(true));
    let error = decompile(&workflow).unwrap_err();
    assert!(error.to_string().contains("hidden"), "{error}");

    workflow.definition.extra.clear();
    workflow
        .definition
        .defs
        .insert("template".into(), runinator_models::json!({}));
    let error = decompile(&workflow).unwrap_err();
    assert!(error.to_string().contains("$defs"), "{error}");
}
