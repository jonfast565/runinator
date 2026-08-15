//! covers `functions.<pkg>.<export>(...)`: how it lowers, what it pins, and that it round-trips.
//!
//! these live here rather than in the codegen crate because they are cross-stage by nature — the
//! call is typed by sema against a synthetic provider, rewritten by lowering, and rendered back by
//! decompile, and only this crate can see all three at once.

use super::*;

use runinator_models::functions::FunctionCatalogEntry;
use runinator_models::providers::{ParameterMetadata, ResultMetadata};
use runinator_models::workflows::WorkflowDefinition;
use uuid::Uuid;

fn catalog_entry(version: i64) -> FunctionCatalogEntry {
    FunctionCatalogEntry {
        package_id: Uuid::from_u128(1),
        package_name: "image_tools".into(),
        namespace: None,
        version_id: Uuid::from_u128(100 + version as u128),
        version,
        export_id: Uuid::from_u128(200 + version as u128),
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

fn function_options(entries: Vec<FunctionCatalogEntry>) -> CompileOptions {
    CompileOptions {
        functions: entries,
        workflow_signatures: test_workflow_signatures(),
        ..CompileOptions::default()
    }
}

const SOURCE: &str = r#"
workflow "Resize" {
    functions.image_tools.resize(source: "a.png", width: 320)
}
"#;

fn compile_functions(src: &str, entries: Vec<FunctionCatalogEntry>) -> WorkflowDefinition {
    compile_str(src, &function_options(entries)).expect("compile packaged function call")
}

fn action_of(definition: &WorkflowDefinition) -> runinator_models::workflows::WorkflowAction {
    definition
        .definition
        .nodes
        .iter()
        .find_map(|node| node.action.clone())
        .expect("an action node")
}

#[test]
fn lowers_to_the_one_provider_and_action_the_runtime_dispatches() {
    let action = action_of(&compile_functions(SOURCE, vec![catalog_entry(3)]));

    // the authored surface names a package and an export; the runtime has one provider with one
    // action, and the export is named by the binding.
    assert_eq!(action.provider, "functions");
    assert_eq!(action.function, "invoke");

    let binding = action.function_binding.expect("a binding");
    assert_eq!(binding.call_path(), "functions.image_tools.resize");
    assert_eq!(binding.version, 3);
    assert_eq!(
        binding.artifact_digest,
        format!("sha256:{}", "a".repeat(64))
    );
}

#[test]
fn the_authored_arguments_become_the_handlers_input() {
    let action = action_of(&compile_functions(SOURCE, vec![catalog_entry(1)]));
    let input = action
        .configuration
        .as_value()
        .get("input")
        .cloned()
        .expect("configuration.input");

    // nested rather than flat so worker staging (`handler`, `package_path`, …) can share the
    // configuration without an author's argument of the same name colliding with it.
    assert_eq!(input.get("source").and_then(Value::as_str), Some("a.png"));
    assert_eq!(input.get("width").and_then(Value::as_i64), Some(320));
    assert!(action.configuration.as_value().get("source").is_none());
}

#[test]
fn an_unversioned_call_pins_the_newest_published_version() {
    let action = action_of(&compile_functions(
        SOURCE,
        vec![catalog_entry(1), catalog_entry(3), catalog_entry(2)],
    ));
    let binding = action.function_binding.expect("a binding");

    // resolved once, at compile time, and recorded. nothing re-resolves it afterwards, which is
    // exactly why moving an alias later cannot change what this workflow calls.
    assert_eq!(binding.version, 3);
}

#[test]
fn packaged_calls_are_routed_to_a_worker_that_can_run_them() {
    let action = action_of(&compile_functions(SOURCE, vec![catalog_entry(1)]));
    // running packaged code needs a container runtime, which not every worker has.
    assert_eq!(
        action.required_labels.get("runner").map(String::as_str),
        Some("functions")
    );
}

#[test]
fn an_explicit_runner_wins_over_the_default() {
    let src = r#"
workflow "Resize" {
    functions.image_tools.resize(source: "a.png").runner("gpu")
}
"#;
    let action = action_of(&compile_functions(src, vec![catalog_entry(1)]));
    // an operator who pinned a pool meant it.
    assert_eq!(
        action.required_labels.get("runner").map(String::as_str),
        Some("gpu")
    );
}

#[test]
fn calling_an_unpublished_function_is_a_compile_error() {
    let error = compile_str(SOURCE, &function_options(Vec::new()))
        .expect_err("an unknown package must not compile");
    let message = error.to_string();
    assert!(message.contains("image_tools"), "{message}");
}

#[test]
fn calling_an_unpublished_export_of_a_published_package_is_a_compile_error() {
    let src = r#"
workflow "Resize" {
    functions.image_tools.crop(source: "a.png")
}
"#;
    let error = compile_str(src, &function_options(vec![catalog_entry(1)]))
        .expect_err("an unknown export must not compile");
    assert!(error.to_string().contains("crop"), "{error}");
}

#[test]
fn round_trips_through_decompile_without_a_catalog() {
    let first = compile_functions(SOURCE, vec![catalog_entry(3)]);
    let wdl = decompile(&first).expect("decompile");

    // the binding carries the authored names, so rendering needs no catalog at all — which is what
    // keeps a definition decompiling the same way after its package is deleted.
    assert!(
        wdl.contains("functions.image_tools.resize("),
        "expected the authored call back, got:\n{wdl}"
    );
    assert!(wdl.contains("source: \"a.png\""), "{wdl}");
    // the implicit runner label is lowering's, not the author's, so it must not reappear as one.
    assert!(!wdl.contains(".runner(\"functions\")"), "{wdl}");

    let second = compile_str(&wdl, &function_options(vec![catalog_entry(3)]))
        .unwrap_or_else(|err| panic!("recompile failed: {err}\n--- decompiled ---\n{wdl}"));
    assert_eq!(
        runinator_workflows::normalize_definition(first.definition.clone()),
        runinator_workflows::normalize_definition(second.definition.clone()),
        "round trip diverged\n--- decompiled ---\n{wdl}"
    );
}

#[test]
fn decompiled_output_is_format_idempotent() {
    let compiled = compile_functions(SOURCE, vec![catalog_entry(3)]);
    let wdl = decompile(&compiled).expect("decompile");
    // the editor pane regenerates through decompile, so a non-idempotent render makes the format
    // button silently rewrite the buffer on every save.
    let formatted = format_str(&wdl).expect("format");
    assert_eq!(wdl.trim(), formatted.trim());
}

#[test]
fn an_ordinary_action_is_untouched_by_any_of_this() {
    let src = r#"
workflow "Plain" {
    github.comments(token: "t", owner: "o", repo: "a", issue_number: "1")
}
"#;
    let options = CompileOptions {
        providers: runinator_provider_catalog::metadata(),
        ..function_options(vec![catalog_entry(1)])
    };
    let action = action_of(&compile_str(src, &options).expect("compile"));
    assert_eq!(action.provider, "github");
    assert_eq!(action.function, "comments");
    assert!(action.function_binding.is_none());
    assert!(action.required_labels.is_empty());
    // arguments stay flat in the configuration, exactly as before packaged functions existed.
    assert_eq!(
        action
            .configuration
            .as_value()
            .get("repo")
            .and_then(Value::as_str),
        Some("a")
    );
}

#[test]
fn a_compensating_packaged_call_is_bound_too() {
    let src = r#"
workflow "Resize" {
    functions.image_tools.resize(source: "a.png")
        compensate functions.image_tools.resize(source: "b.png")
}
"#;
    let definition = compile_functions(src, vec![catalog_entry(2)]);
    let compensation = definition
        .definition
        .nodes
        .iter()
        .find_map(|node| node.compensation.clone())
        .expect("a compensation action");

    // easy to miss: compensation lowers through its own path, and a compensating packaged call is
    // as much a packaged call as the forward one.
    assert_eq!(compensation.provider, "functions");
    assert_eq!(compensation.function, "invoke");
    assert_eq!(
        compensation.function_binding.map(|binding| binding.version),
        Some(2)
    );
}

#[test]
fn the_synthetic_provider_types_the_call_against_the_published_signature() {
    let options = function_options(vec![catalog_entry(1)]);
    let providers = options.function_providers();

    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].name, "functions.image_tools");
    assert_eq!(providers[0].actions.len(), 1);
    assert_eq!(providers[0].actions[0].function_name, "resize");
    // packaged code runs a container, so it can never be evaluated in the reducer.
    assert!(!providers[0].actions[0].pure);
}

#[test]
fn several_versions_of_one_export_present_as_a_single_action() {
    let options = function_options(vec![catalog_entry(1), catalog_entry(3), catalog_entry(2)]);
    let providers = options.function_providers();

    // the type checker must see exactly what lowering will bind: one action, the newest version.
    assert_eq!(providers[0].actions.len(), 1);
    assert_eq!(
        options
            .resolve_function("functions.image_tools", "resize")
            .map(|entry| entry.version),
        Some(3)
    );
}

#[test]
fn a_namespaced_package_keeps_its_namespace_in_the_call_path() {
    let mut entry = catalog_entry(1);
    entry.namespace = Some("media".into());
    let src = r#"
workflow "Resize" {
    functions.media.image_tools.resize(source: "a.png")
}
"#;
    let action = action_of(&compile_functions(src, vec![entry]));
    assert_eq!(
        action.function_binding.map(|binding| binding.call_path()),
        Some("functions.media.image_tools.resize".to_string())
    );
}

#[test]
fn a_function_catalog_alone_does_not_make_a_compile_strict() {
    // sema is permissive about unknown actions when no provider catalog was supplied, which is what
    // lets a pack compile offline. the synthetic `functions.<pkg>` providers are derived from the
    // caller's function list, so counting them as a catalog would silently reject every ordinary
    // action in any pack that gained one packaged function.
    let src = r#"
workflow "Plain" {
    some_unknown_provider.some_action(a: 1)
}
"#;
    compile_str(src, &function_options(vec![catalog_entry(1)]))
        .expect("an unknown provider stays permissive when only functions were supplied");

    // and with a real catalog present it is strict again, as it always was.
    let options = CompileOptions {
        providers: runinator_provider_catalog::metadata(),
        ..function_options(vec![catalog_entry(1)])
    };
    compile_str(src, &options).expect_err("a real catalog still rejects an unknown action");
}

#[test]
fn a_hyphenated_package_name_is_callable() {
    // manifests allow `-` in a package name (`image-tools` is the checked-in example), so the call
    // path has to survive it. this is the one character that could plausibly parse as an operator.
    let mut entry = catalog_entry(1);
    entry.package_name = "image-tools".into();
    let src = r#"
workflow "Resize" {
    functions.image-tools.resize(source: "a.png")
}
"#;
    let action = action_of(&compile_functions(src, vec![entry]));
    assert_eq!(
        action.function_binding.map(|binding| binding.call_path()),
        Some("functions.image-tools.resize".to_string())
    );
}
