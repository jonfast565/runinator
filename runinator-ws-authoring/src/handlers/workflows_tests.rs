//! workflow HTTP authoring boundary tests.

use super::*;

fn workflow() -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: "portable".into(),
        key: Some("portable".into()),
        namespace: Some("runinator.tests".into()),
        org_id: None,
        version: Default::default(),
        enabled: true,
        input_type: Default::default(),
        output_type: Default::default(),
        definition: Default::default(),
        created_at: None,
        updated_at: None,
    }
}

#[test]
fn accepts_only_rexrap_portable_graph_sidecars() {
    let mut portable = workflow();
    portable
        .definition
        .extra
        .insert("ui".into(), runinator_models::json!({ "layout": {} }));
    portable.definition.metadata = runinator_models::json!({ "owner_note": "portable" });
    assert_eq!(rexrap_boundary_error(&portable), None);

    let mut derived = workflow();
    derived.definition.metadata = runinator_models::json!({ "artifact_refs": { "settings": [] } });
    assert_eq!(rexrap_boundary_error(&derived), None);

    let mut definitions = workflow();
    definitions
        .definition
        .defs
        .insert("template".into(), runinator_models::json!({}));
    assert!(
        rexrap_boundary_error(&definitions)
            .expect("$defs rejected")
            .contains("$defs")
    );

    let mut extra = workflow();
    extra
        .definition
        .extra
        .insert("hidden".into(), Value::Bool(true));
    assert!(
        rexrap_boundary_error(&extra)
            .expect("unknown extra rejected")
            .contains("hidden")
    );

    let mut managed = workflow();
    managed.definition.metadata = runinator_models::json!({ "managed_by": "functions" });
    assert!(
        rexrap_boundary_error(&managed)
            .expect("managed marker rejected")
            .contains("managed_by")
    );
}
