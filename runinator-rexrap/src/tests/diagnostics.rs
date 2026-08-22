//! error reporting: which subexpression a span blames, how a rendered error reads, and the full
//! diagnostic set `analyze_source` returns.

use super::*;

#[test]
fn semantic_error_span_points_at_subexpression() {
    let src = r#"
        workflow "Bad" v1 {
            params { a: string }

            do {
                console.run(command: params.b)
            }
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("unknown field 'b'"), "{message}");
    // the span is the path expression, not the whole statement.
    assert_eq!(&src[span.start..span.end], "params.b", "span = {span:?}");
}
#[test]
fn unorderable_comparison_blames_the_operand() {
    let src = r#"
        workflow "Bad" v1 {
            params { flag: boolean }

            do {
                if params.flag > 0 { console.run(command: "y") }
            }
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("cannot order"), "{message}");
    // the left operand is blamed, not the enclosing if statement.
    assert_eq!(&src[span.start..span.end], "params.flag", "span = {span:?}");
}
#[test]
fn unknown_reference_blames_the_path() {
    let src = r#"
        workflow "Bad" v1 {

            do {
                console.run(command: ghost.value)
            }
        }
    "#;
    let (span, message) = expect_semantic(src);
    assert!(message.contains("unknown reference 'ghost'"), "{message}");
    assert_eq!(&src[span.start..span.end], "ghost.value", "span = {span:?}");
}
#[test]
fn renders_semantic_error_with_caret() {
    let src = r#"
        workflow "Bad" v1 {
            params { a: string }

            do {
                console.run(command: params.b)
            }
        }
    "#;
    let err = compile_str(src, &CompileOptions::default()).unwrap_err();
    let rendered = err.render(src);
    assert!(rendered.contains("error:"), "{rendered}");
    assert!(rendered.contains("^"), "{rendered}");
    // `params.b` sits on the sixth line of the raw string literal.
    assert!(rendered.contains("line 6"), "{rendered}");
}
#[test]
fn analyze_source_reports_all_diagnostics() {
    let src = r#"
        workflow "Bad" v1 {
            params { a: string }

            do {
                console.run(command: params.b)
                console.run(command: params.c)
            }
        }
    "#;
    let diagnostics = analyze_source(src).expect("parse");
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    // both unknown-field accesses surface, not just the first.
    assert_eq!(errors.len(), 2, "{diagnostics:?}");
    assert!(errors[0].render(src).contains("^"));
}

// parse-time rich errors -----------------------------------------------------
#[test]
fn rejects_removed_source_forms() {
    for src in [
        r#"workflow "Old" v1 { do { node x = console.run(command: "x") } }"#,
        r#"workflow "Old" v1 { do {output "done" { ok: true } }}"#,
        r#"workflow "Old" v1 { do {let child = call "Child" with { id: "1" } }}"#,
        r#"workflow "Old" v1 { do {node spawn "Child" with { id: "1" } }}"#,
        r#"workflow "Old" v1 { namespace old.inside }"#,
    ] {
        assert!(
            compile_str(src, &CompileOptions::default()).is_err(),
            "{src}"
        );
    }
}
#[test]
fn accepts_document_level_and_namespace_block_workflows() {
    let src = r#"
        namespace core.sdlc
        workflow "First" v1

        params { id: string }

        do {
            let one = console.run(command: params.id)
        }

        namespace core.more {
            workflow "Second" v1 {

                do {
                    let two = compute {
                        return "ok"
                    }
                }
            }
        }
    "#;
    let definitions = compile_all_str(src, &default_test_options()).expect("compile all");
    assert_eq!(definitions.len(), 2);
    assert_eq!(definitions[0].namespace.as_deref(), Some("core.sdlc"));
    assert_eq!(definitions[0].name, "First");
    assert_eq!(definitions[1].namespace.as_deref(), Some("core.more"));
    assert_eq!(definitions[1].name, "Second");
}
#[test]
fn bound_control_region_yields_collected_value() {
    let src = r#"
        workflow "Functional" v1 {
            params { flag: boolean }

            do {
                let decision = if params.flag {
                    yield { status: "ready" }
                } else {
                    yield { status: "empty" }
                }
                let report = console.run(command: decision.status)
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let nodes = graph["nodes"].as_array().expect("nodes");
    assert!(nodes.iter().any(|node| node["id"] == "decision"));
}
#[test]
#[ignore = "invocation output type hint migration pending"]
fn bound_parallel_collects_functional_value_shape() {
    let src = r#"
        workflow "Parallel Value" v1 {

            do {
                let joined = parallel {
                    branch {
                        console.run(command: "a")
                    }
                    branch {
                        console.run(command: "b")
                    }
                } join all
                let report = console.run(command: string(joined.outputs))
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let collector = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "joined")
        .expect("bound parallel collector");
    let program = collector["action"]["configuration"]["program"].to_string();
    assert!(program.contains("\"branches\""), "program: {program}");
    assert!(program.contains("\"wait_for\""), "program: {program}");
    assert!(program.contains("\"outputs\""), "program: {program}");
}
#[test]
fn unknown_modifier_is_a_syntax_error_with_span() {
    let src = r#"
        workflow "Bad" v1 {

            do {
                @bogus(1)
                console.run(command: "x")
            }
        }
    "#;
    match parse_document(src) {
        Err(RexRapError::Syntax { span, message }) => {
            assert!(message.contains("unknown attribute '@bogus'"), "{message}");
            assert!(span.end > span.start, "empty span {span:?}");
        }
        other => panic!("expected syntax error, got {other:?}"),
    }
}
