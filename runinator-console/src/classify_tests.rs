//! covers the one decision this crate exists to make: pure in-process, or a workflow run.
//!
//! the direction that matters is the fallback. a cell wrongly classified as pure would run a
//! provider action inside an http handler — no run to record it, no retry, no timeout, no cancel —
//! so every case that is not provably a pure fragment has to land on `Workflow`.

use super::*;

fn options() -> CompileOptions {
    CompileOptions::default()
}

fn kind(source: &str) -> CellKind {
    classify(source, &options())
        .unwrap_or_else(|err| panic!("classify({source:?}) failed: {err}"))
        .kind
}

#[test]
fn a_bare_expression_is_pure() {
    assert_eq!(kind("1 + 2"), CellKind::Expression);
    assert_eq!(kind("\"hello\""), CellKind::Expression);
    // a reference to an earlier cell is still just an expression.
    assert_eq!(kind("cells.load.rows"), CellKind::Expression);
}

#[test]
fn a_pure_intrinsic_call_is_pure() {
    assert_eq!(kind("std.strings.upper(\"abc\")"), CellKind::Expression);
}

#[test]
fn an_action_call_becomes_a_workflow() {
    // the important direction: this must never be evaluated in the web service.
    assert_eq!(
        kind("github.comments(token: \"t\", owner: \"o\", repo: \"r\", issue_number: \"1\")"),
        CellKind::Workflow
    );
}

#[test]
fn control_flow_becomes_a_workflow() {
    assert_eq!(
        kind("if params.enabled == true {\n    console.run(command: \"go\")\n}"),
        CellKind::Workflow
    );
}

#[test]
fn several_statements_become_a_workflow() {
    assert_eq!(
        kind("console.run(command: \"one\")\nconsole.run(command: \"two\")"),
        CellKind::Workflow
    );
}

#[test]
fn an_empty_cell_is_refused_rather_than_run() {
    assert!(matches!(
        classify("   \n  ", &options()),
        Err(ConsoleError::Empty)
    ));
    // a cell of only comments is empty too, or an accidental "run" would start a scratch workflow
    // that does nothing and leaves a row behind.
    assert!(matches!(
        classify("// just a note\n// and another", &options()),
        Err(ConsoleError::Empty)
    ));
}

#[test]
fn a_pure_cell_carries_its_lowered_form_and_a_workflow_cell_its_source() {
    let expression = classify("1 + 2", &options()).unwrap();
    assert!(expression.is_pure());
    assert!(expression.lowered.is_some());
    assert!(expression.workflow_source.is_none());
    assert_eq!(
        expression.fragment_kind(),
        Some(runinator_rexrap::RexRapFragmentKind::Expression)
    );

    let workflow = classify("console.run(command: \"go\")", &options()).unwrap();
    assert!(!workflow.is_pure());
    assert!(workflow.lowered.is_none());
    assert_eq!(
        workflow.workflow_source.as_deref(),
        Some("console.run(command: \"go\")")
    );
    assert_eq!(workflow.fragment_kind(), None);
}

#[test]
fn a_cell_is_wrapped_into_a_workflow_unless_it_declares_one() {
    let wrapped = workflow_source("console.run(command: \"go\")", "console.abc");
    assert!(wrapped.starts_with("namespace runinator.console {"));
    assert!(wrapped.contains("workflow \"console.abc\" v1 {"));
    assert!(wrapped.contains("key console.console_abc"));
    assert!(wrapped.contains("    console.run(command: \"go\")"));

    // an author who wrote their own workflow block meant it; wrapping again would nest one.
    let authored =
        "workflow \"Mine\" v1 {\n    do {\n        console.run(command: \"go\")\n    }\n}";
    assert_eq!(workflow_source(authored, "console.abc"), authored);
}

#[test]
fn a_commented_out_workflow_keyword_does_not_look_like_a_declaration() {
    // otherwise the cell would be passed through unwrapped and compile to nothing.
    let source = "// workflow \"Old\" v1 {\nconsole.run(command: \"go\")";
    let wrapped = workflow_source(source, "console.abc");
    assert!(
        wrapped.starts_with("namespace runinator.console {"),
        "{wrapped}"
    );
}

#[test]
fn console_accepts_function_library_modules_and_full_documents() {
    assert_eq!(
        kind("fn double(x: integer) -> integer = x * 2"),
        CellKind::Library
    );
    assert_eq!(
        kind("fn double(x: integer) -> integer = x * 2\ndo {\n    console.run(command: \"go\")\n}"),
        CellKind::Workflow
    );
    assert_eq!(
        kind(
            "workflow \"Mine\" v1 {\n    do {\n        console.run(command: \"go\")\n    }\n}\nfn double(x: integer) -> integer = x * 2"
        ),
        CellKind::Workflow
    );
    assert!(matches!(
        classify(
            "workflow \"One\" v1 { do {} }\nworkflow \"Two\" v1 { do {} }",
            &options()
        ),
        Err(ConsoleError::Uncompilable(_))
    ));
}

#[test]
fn bare_runtime_do_is_wrapped_once_and_compute_keyword_is_pure() {
    let source = "do {\n    console.run(command: \"go\")\n}";
    assert_eq!(kind(source), CellKind::Workflow);
    let wrapped = workflow_source(source, "console.abc");
    assert!(
        wrapped.contains("workflow \"console.abc\" v1 {\n    key console.console_abc\n    do {"),
        "{wrapped}"
    );
    assert!(!wrapped.contains("do {\n        do {"), "{wrapped}");
    assert_eq!(kind("compute { return 1 + 2 }"), CellKind::Do);
}
