//! `decompile_with_spans`: every graph node paired with the text that produced it.
//!
//! The contract these pin is that a span indexes the *returned* text — the round trip through the
//! formatter is exactly what makes a naive decompiler-internal offset wrong.

use super::*;

fn compile_one(src: &str) -> runinator_models::workflows::WorkflowDefinition {
    compile_str(src, &CompileOptions::default()).unwrap()
}

#[test]
fn every_span_indexes_the_returned_text_and_names_its_own_node() {
    let definition = compile_one(
        r#"
        workflow "Spans" v1 {
            do {
                let first = console.run(command: "one")
                let second = console.run(command: "two")
            }
        }
        "#,
    );

    let (text, spans) = decompile_with_spans(&definition).unwrap();
    assert!(!spans.is_empty(), "a two-statement workflow has spans");

    for span in &spans {
        assert!(
            span.start < span.end && span.end <= text.len(),
            "span {span:?} is not a valid range into {} bytes of text",
            text.len()
        );
        // the slice a highlight would show must actually be the statement that made the node.
        let slice = &text[span.start..span.end];
        assert!(
            slice.contains(&span.node_id),
            "span for {} points at {slice:?}, which does not mention it",
            span.node_id
        );
    }
}

#[test]
fn spans_cover_the_nodes_the_graph_actually_has() {
    let definition = compile_one(
        r#"
        workflow "Covered" v1 {
            do {
                let only = console.run(command: "one")
            }
        }
        "#,
    );

    let (_, spans) = decompile_with_spans(&definition).unwrap();
    assert!(
        spans.iter().any(|span| span.node_id == "only"),
        "the authored label is a node id and must have a span: {spans:?}"
    );
    // start/end/fail are synthesized outside any statement, so they deliberately have none.
    for generated in ["start", "end", "fail"] {
        assert!(
            !spans.iter().any(|span| span.node_id == generated),
            "{generated} is not authored text and must not claim a span"
        );
    }
}

#[test]
fn a_nested_statement_gets_its_own_span_inside_its_parent() {
    let definition = compile_one(
        r#"
        workflow "Nested" v1 {
            do {
                let gate = console.run(command: "check")
                if gate.ok {
                    let inner = console.run(command: "yes")
                }
            }
        }
        "#,
    );

    let (text, spans) = decompile_with_spans(&definition).unwrap();
    let inner = spans
        .iter()
        .find(|span| span.node_id == "inner")
        .expect("the nested statement has a span");
    // the formatter may wrap the call across lines, so assert what is actually invariant: the span
    // is the nested statement, not its enclosing `if`.
    assert!(
        text[inner.start..inner.end]
            .trim_start()
            .starts_with("let inner"),
        "expected the nested statement, got {:?}",
        &text[inner.start..inner.end]
    );
    // and it sits strictly inside the branch node that encloses it.
    assert!(
        spans.iter().any(|outer| outer.node_id != inner.node_id
            && outer.start <= inner.start
            && inner.end <= outer.end),
        "the nested span must be contained by its parent statement's span: {spans:?}"
    );
}
