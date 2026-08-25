//! covers the console's scope semantics, which is the part a user actually feels: what a cell binds,
//! what it does not, and what a later cell can see.
//!
//! execution itself is covered where it belongs — the classifier in `runinator-console`, the
//! evaluator in `runinator-rexrap`, and the run path by the WS behavior suite against a real database.

use super::*;

use runinator_console::{CELL_SCOPE, CONTEXT_ROOT, ConsoleContext, cell_binding_name};
use runinator_database::sqlite::SqliteDb;
use runinator_models::console::{
    ConsoleCellKind, ConsoleCellStatus, ConsoleFunction, NewConsoleCell,
};
use runinator_models::providers::{
    ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata, RuninatorType,
};
use runinator_models::workflows::WorkflowNodeKind;
use runinator_store::{DatabaseImpl, roles::DefinitionStore};
use uuid::Uuid;

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
    let mut definition = runinator_rexrap::compile_str(
        "workflow \"console.abc.def\" v1 {\n    do {\n        console.run(command: \"go\")\n    }\n}\n",
        &runinator_rexrap::CompileOptions {
            enabled: true,
            ..runinator_rexrap::CompileOptions::default()
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

#[test]
fn a_session_task_function_call_compiles_to_the_normal_provider_action() {
    // Session declarations are spliced into the same scratch document a provider call already
    // uses; their bodies must lower to graph actions, never an evaluator-side shortcut.
    let function = ConsoleFunction {
        id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        cell_id: Uuid::new_v4(),
        name: "deploy".into(),
        is_task: true,
        source: "task fn deploy(command: string) do {\n    let output = console.run(command: command)\n}".into(),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let options = runinator_rexrap::CompileOptions {
        enabled: true,
        ..runinator_rexrap::CompileOptions::default()
    };

    let classification = runinator_console::classify_with_functions(
        "deploy(command: \"release\")",
        &options,
        std::slice::from_ref(&function),
    )
    .expect("task function call classifies");
    assert_eq!(classification.kind, runinator_console::CellKind::Workflow);

    let source = runinator_console::workflow_source_with_functions(
        "deploy(command: \"release\")",
        "console.task-function",
        &[function],
    );
    let definition = runinator_rexrap::compile_str(&source, &options)
        .expect("session task function scratch workflow compiles");
    assert!(definition.definition.nodes.iter().any(|node| {
        node.kind == WorkflowNodeKind::Action
            && node
                .action
                .as_ref()
                .is_some_and(|action| action.provider == "console" && action.function == "run")
    }));
}

#[tokio::test]
async fn a_session_task_function_starts_a_durable_scratch_run() {
    let path =
        std::env::temp_dir().join(format!("runinator-console-task-fn-{}.db", Uuid::now_v7()));
    let db = SqliteDb::new(path.to_str().expect("temporary database path"))
        .await
        .expect("opens sqlite database");
    db.run_init_scripts(&Vec::new())
        .await
        .expect("applies migrations");

    let session = create_session(&db, None, "task function", None)
        .await
        .expect("creates console session");
    let library = upsert_cell(
        &db,
        session.id,
        None,
        &NewConsoleCell {
            source: "task fn deploy(command: string) do {\n    let output = console.run(command: command)\n}".into(),
            label: None,
            position: None,
        },
    )
    .await
    .expect("stores library cell");
    let published = run_cell(&db, library.id, Vec::new(), Vec::new())
        .await
        .expect("publishes task function");
    assert_eq!(published.cell.kind, Some(ConsoleCellKind::Library));
    assert_eq!(published.cell.status, ConsoleCellStatus::Succeeded);

    let invocation = upsert_cell(
        &db,
        session.id,
        None,
        &NewConsoleCell {
            source: "deploy(command: \"release\")".into(),
            label: None,
            position: None,
        },
    )
    .await
    .expect("stores invocation cell");
    let console_provider = ProviderMetadata {
        name: "console".into(),
        actions: vec![ActionMetadata::new("run", "run").with_parameters(vec![
            ParameterMetadata::required("command", RuninatorType::Any),
        ])],
        metadata: ProviderRuntimeMetadata::default(),
    };
    db.upsert_catalog_item(crate::repository::provider_catalog_item(&console_provider))
        .await
        .expect("registers provider metadata for durable definition validation");
    let outcome = run_cell(&db, invocation.id, vec![console_provider], Vec::new())
        .await
        .expect("starts scratch workflow");
    assert_eq!(
        outcome.cell.status,
        ConsoleCellStatus::Running,
        "scratch compilation failed: {:?}",
        outcome.cell.error
    );
    let run = outcome.run.expect("task function invocation becomes a run");
    assert!(run.workflow_snapshot.is_some_and(|definition| {
        definition.definition.nodes.iter().any(|node| {
            node.kind == WorkflowNodeKind::Action
                && node
                    .action
                    .as_ref()
                    .is_some_and(|action| action.provider == "console" && action.function == "run")
        })
    }));

    drop(db);
    let _ = std::fs::remove_file(path);
}
