//! what the compiler refuses: unknown references and targets, out-of-scope loop vars, duplicate
//! ids, and the warnings that are not errors.

use super::*;

#[test]
fn rejects_unknown_reference() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            console.run(command: ghost.value)
        }
    "#,
    );
    assert!(message.contains("unknown reference 'ghost'"), "{message}");
}
#[test]
fn rejects_unknown_transition_target() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            console.run(command: "x") -> ghost
        }
    "#,
    );
    assert!(message.contains("unknown step 'ghost'"), "{message}");
}
#[test]
fn rejects_unknown_input_field() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { a: string }
            console.run(command: params.b)
        }
    "#,
    );
    assert!(message.contains("unknown field 'b'"), "{message}");
}
#[test]
fn rejects_non_array_for_source() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { n: integer }
            for x in params.n { console.run(command: "y") }
        }
    "#,
    );
    assert!(message.contains("expects an array"), "{message}");
}
#[test]
fn rejects_unorderable_comparison() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { flag: boolean }
            if params.flag > 0 { console.run(command: "y") }
        }
    "#,
    );
    assert!(message.contains("cannot order"), "{message}");
}
#[test]
fn rejects_loop_var_out_of_scope() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            params { items: string[] }
            for x in params.items { console.run(command: "in") }
            console.run(command: x)
        }
    "#,
    );
    assert!(message.contains("unknown reference 'x'"), "{message}");
}
#[test]
fn rejects_duplicate_node_id() {
    let message = expect_semantic_error(
        r#"
        workflow "Bad" v1 {
            node foo <- console.run(command: "a")
            node foo <- console.run(command: "b")
        }
    "#,
    );
    assert!(message.contains("duplicate node id 'foo'"), "{message}");
}
#[test]
fn warns_on_unreachable_after_fail() {
    let src = r#"
        workflow "Dead" v1 {
            console.run(command: "ok")
            fail "boom"
            console.run(command: "never")
        }
    "#;
    let (_, warnings) =
        compile_str_with_diagnostics(src, &CompileOptions::default()).expect("compile");
    assert!(
        warnings.iter().any(|w| w.message.contains("unreachable")),
        "expected unreachable warning, got {warnings:?}"
    );
}
