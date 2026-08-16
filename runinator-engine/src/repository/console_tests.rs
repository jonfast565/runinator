//! covers the console's scope semantics, which is the part a user actually feels: what a cell binds,
//! what it does not, and what a later cell can see.
//!
//! execution itself is covered where it belongs — the classifier in `runinator-console`, the
//! evaluator in `runinator-wdl`, and the run path by the ws behavior suite against a real database.

use super::*;

use runinator_console::{CELL_SCOPE, CONTEXT_ROOT, ConsoleContext, cell_binding_name};

#[test]
fn a_later_cell_sees_an_earlier_ones_result() {
    let mut context = ConsoleContext::new();
    context.bind("load", runinator_models::json!({ "rows": [1, 2, 3] }));
    let value = context.as_value();

    // the scope is what makes a notebook a notebook rather than a list of unrelated snippets.
    assert_eq!(
        value.pointer("/input/load/rows/0").and_then(Value::as_i64),
        Some(1)
    );
}

#[test]
fn bindings_are_namespaced_so_a_cell_cannot_shadow_a_workflow_root() {
    let mut context = ConsoleContext::new();
    // a cell labelled `config` must not become the real `config` root, which resolves settings.
    // the scope is `params`, so it lands at `params.config` and shadows nothing.
    context.bind("config", runinator_models::json!({ "hijacked": true }));
    let value = context.as_value();

    assert!(value.get("config").is_none());
    assert!(
        value
            .pointer("/input/config/hijacked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );
    // the surface name and the context key differ; both are pinned so neither drifts alone.
    assert_eq!(CELL_SCOPE, "params");
    assert_eq!(CONTEXT_ROOT, "input");
}

#[test]
fn an_unlabelled_cell_binds_by_position() {
    // so every cell is referenceable without forcing an author to name each one.
    assert_eq!(cell_binding_name(None, 0), "cell_0");
    assert_eq!(cell_binding_name(Some("total"), 0), "total");
}

#[test]
fn a_console_scratch_workflow_is_recognisable_as_managed() {
    // built through the compiler rather than hand-assembled, so the marker is checked against a
    // definition of the shape the scratch path actually produces.
    let mut definition = runinator_wdl::compile_str(
        "workflow \"console.abc.def\" v1 {\n    console.run(command: \"go\")\n}\n",
        &runinator_wdl::CompileOptions {
            enabled: true,
            ..runinator_wdl::CompileOptions::default()
        },
    )
    .expect("scratch workflow compiles");
    assert!(!is_console_workflow(&definition));

    stamp_managed(&mut definition);
    // marked so it is filtered out of the workflow list exactly as a function adapter is: one
    // scratch workflow per cell run would otherwise bury every authored workflow.
    assert!(is_console_workflow(&definition));
    assert_eq!(
        definition
            .definition
            .metadata
            .get("managed_by")
            .and_then(Value::as_str),
        Some("console")
    );
}
