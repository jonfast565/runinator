//! covers the generated adapter workflow: that it compiles at all, that it is marked generated, and
//! that its shape is the one the invocation path depends on.
//!
//! generating it by compiling WDL rather than assembling graph json is the reason these are cheap —
//! if the source is wrong the compiler says so here, rather than a malformed graph reaching the
//! reducer at invocation time.

use super::*;

use runinator_models::functions::{FunctionResourceLimits, FunctionRuntimeSpec};
use runinator_models::providers::{ParameterMetadata, ResultMetadata};

fn entry() -> FunctionCatalogEntry {
    FunctionCatalogEntry {
        package_id: Uuid::from_u128(1),
        package_name: "image_tools".into(),
        namespace: None,
        version_id: Uuid::from_u128(2),
        version: 3,
        export_id: Uuid::from_u128(3),
        export_name: "resize".into(),
        artifact_digest: format!("sha256:{}", "a".repeat(64)),
        description: Some("resize an image".into()),
        input: vec![
            ParameterMetadata::required("source", RuninatorType::String),
            ParameterMetadata::optional("width", RuninatorType::Integer),
        ],
        output: vec![ResultMetadata::new("uri", RuninatorType::String)],
        aliases: vec!["latest".into()],
    }
}

fn export() -> FunctionExport {
    FunctionExport {
        id: Uuid::from_u128(3),
        version_id: Uuid::from_u128(2),
        name: "resize".into(),
        handler: "src.images.resize".into(),
        description: Some("resize an image".into()),
        input: entry().input,
        output: entry().output,
        limits: FunctionResourceLimits::default(),
    }
}

#[test]
fn generates_a_workflow_that_calls_the_pinned_version() {
    let definition = build_adapter_workflow(&entry(), &export()).expect("adapter compiles");

    assert_eq!(definition.name, "functions.image_tools.resize");
    let action = definition
        .definition
        .nodes
        .iter()
        .find_map(|node| node.action.clone())
        .expect("an action node");

    // it goes through exactly the same lowering as an authored call, so it carries the same binding
    // — which is what makes the http path and the workflow path run identical code.
    assert_eq!(action.provider, "functions");
    assert_eq!(action.function, "invoke");
    let binding = action.function_binding.expect("a binding");
    assert_eq!(binding.version, 3);
    assert_eq!(binding.export_id, Uuid::from_u128(3));
    assert_eq!(
        action.required_labels.get("runner").map(String::as_str),
        Some("functions")
    );
}

#[test]
fn the_exports_declared_inputs_become_workflow_params() {
    let definition = build_adapter_workflow(&entry(), &export()).expect("adapter compiles");

    // so an http request body is validated against the same schema a workflow call is typed
    // against, rather than a second hand-maintained copy of it.
    let RuninatorType::Struct { fields, .. } = &definition.input_type else {
        panic!("expected a struct input, got {:?}", definition.input_type);
    };
    assert!(fields.contains_key("source"));
    assert!(fields.contains_key("width"));
}

#[test]
fn the_generated_workflow_is_marked_managed() {
    let definition = build_adapter_workflow(&entry(), &export()).expect("adapter compiles");

    assert!(is_adapter_workflow(&definition));
    assert_eq!(
        definition
            .definition
            .metadata
            .get("namespace")
            .and_then(Value::as_str),
        Some("functions")
    );
    // the export id is recorded so the invocation path can get back to what it stands for.
    assert_eq!(
        definition
            .definition
            .metadata
            .pointer("/function/export_id")
            .and_then(Value::as_str)
            .and_then(|raw| raw.parse::<Uuid>().ok()),
        Some(Uuid::from_u128(3))
    );
}

#[test]
fn an_authored_workflow_is_not_mistaken_for_an_adapter() {
    let mut definition = build_adapter_workflow(&entry(), &export()).expect("adapter compiles");
    definition.definition.metadata = Value::Object(Default::default());
    assert!(!is_adapter_workflow(&definition));
}

#[test]
fn an_export_with_no_inputs_still_compiles() {
    let mut entry = entry();
    entry.input = Vec::new();
    let mut export = export();
    export.input = Vec::new();

    // an empty `params { }` block is not valid wdl, so the generator has to omit it entirely —
    // easy to get wrong, and it would take down publishing for any zero-argument export.
    let definition = build_adapter_workflow(&entry, &export).expect("adapter compiles");
    assert!(
        definition
            .definition
            .nodes
            .iter()
            .any(|node| node.action.is_some())
    );
}

#[test]
fn a_structural_input_degrades_to_any_rather_than_failing_the_publish() {
    let mut entry = entry();
    entry.input = vec![ParameterMetadata::required(
        "options",
        RuninatorType::Array(Box::new(RuninatorType::String)),
    )];
    let mut export = export();
    export.input = entry.input.clone();

    // the export's real schema still types the call; an adapter that refused to compile over a
    // shape it could not spell would take the whole publish with it.
    build_adapter_workflow(&entry, &export).expect("adapter compiles");
}

#[test]
fn a_namespaced_package_names_its_adapter_with_the_namespace() {
    let mut entry = entry();
    entry.namespace = Some("media".into());
    assert_eq!(
        adapter_workflow_name(&entry),
        "functions.media.image_tools.resize"
    );
    // and it still compiles, which is what proves the three-segment provider survives generation.
    let definition = build_adapter_workflow(&entry, &export()).expect("adapter compiles");
    assert_eq!(definition.name, "functions.media.image_tools.resize");
}

// silence the unused-import warning when only some fixtures are used.
#[allow(dead_code)]
fn runtime_spec() -> FunctionRuntimeSpec {
    FunctionRuntimeSpec::new("python3.13")
}
