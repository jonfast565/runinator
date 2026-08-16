//! the assembler, checked against the evaluator it has to agree with.
//!
//! the assertion that matters is not "these instructions look right" — it is that assembling a
//! program and running it on the vm produces what running the same program on the tree evaluator
//! produces. two implementations of one language is exactly the situation where an isolated unit
//! test passes while the halves disagree, so most of these tests compare the two directly.

use super::*;

use crate::compute::ComputeOutcome;
use crate::compute::{PureIntrinsics, parse_program, run_program_with};
use runinator_models::invocation::{InvocationModule, InvocationStep};
use runinator_models::json;

fn catalog() -> CallableCatalog {
    CallableCatalog::builtin()
}

/// run a lowered program both ways and assert they agree, returning the shared value.
fn agree(program: &Value, context: &Value) -> Value {
    let parsed = parse_program(program).expect("the program parses");

    let evaluated = match run_program_with(&parsed, context, &PureIntrinsics, None) {
        Ok(ComputeOutcome::Return(value)) | Ok(ComputeOutcome::Fallthrough(value)) => value,
        Ok(ComputeOutcome::Goto(target)) => panic!("unexpected goto to '{target}'"),
        Err(err) => panic!("the evaluator rejected the program: {err}"),
    };

    let module = InvocationModule::new(assemble_program(&parsed, &catalog()).expect("assembles"));
    let catalog = catalog();
    let env = crate::vm::VmEnv::pure(context, &catalog);
    let assembled = match crate::vm::start(&module, &env) {
        InvocationStep::Complete { value } => value,
        other => panic!("the vm did not complete: {other:?}"),
    };

    assert_eq!(
        evaluated, assembled,
        "the evaluator and the vm disagreed about this program"
    );
    assembled
}

#[test]
fn a_literal_return_agrees() {
    assert_eq!(
        agree(&json!([{ "$return": 7 }]), &json!({})),
        Value::from(7)
    );
}

#[test]
fn let_bindings_are_visible_to_later_statements() {
    let program = json!([
        { "$let": "x", "value": 2 },
        { "$return": { "$call": "add", "args": [{ "$ref": { "let": ["x"] } }, 3] } },
    ]);
    assert_eq!(agree(&program, &json!({})), Value::from(5));
}

#[test]
fn refs_resolve_against_the_context() {
    let program = json!([{ "$return": { "$ref": { "input": ["name"] } } }]);
    let context = json!({ "input": { "name": "ada" } });
    assert_eq!(agree(&program, &context), Value::from("ada"));
}

#[test]
fn arithmetic_folds_left_to_right_like_the_evaluator() {
    // three operands is the case a single n-ary call would get wrong: the library's `add` is binary,
    // so the assembler has to fold.
    let program = json!([{ "$return": { "$sub": [10, 3, 2] } }]);
    assert_eq!(agree(&program, &json!({})), Value::from(5));
}

#[test]
fn integer_arithmetic_stays_integral() {
    let program = json!([{ "$return": { "$div": [7, 2] } }]);
    // integer division, not 3.5 — this is the evaluator's rule and the vm must not widen it.
    assert_eq!(agree(&program, &json!({})), Value::from(3));
}

#[test]
fn concat_requires_strings_in_both_implementations() {
    let program = json!([{ "$return": { "$concat": ["a", "b", "c"] } }]);
    assert_eq!(agree(&program, &json!({})), Value::from("abc"));
}

#[test]
fn coalesce_takes_the_first_non_null() {
    let program = json!([{ "$return": { "$coalesce": [null, null, "fallback"] } }]);
    assert_eq!(agree(&program, &json!({})), Value::from("fallback"));
}

#[test]
fn coalesce_does_not_evaluate_past_the_first_non_null() {
    // the right-hand operand would fail if it were evaluated, so this passing is the proof that the
    // assembler compiled `??` to a jump rather than to an eager call.
    let program = json!([{
        "$return": { "$coalesce": ["here", { "$to_string": [] }] }
    }]);
    let parsed = parse_program(&program).expect("parses");
    let module = InvocationModule::new(assemble_program(&parsed, &catalog()).expect("assembles"));
    let catalog = catalog();
    let context = json!({});
    match crate::vm::start(&module, &crate::vm::VmEnv::pure(&context, &catalog)) {
        InvocationStep::Complete { value } => assert_eq!(value, Value::from("here")),
        other => panic!("coalesce evaluated its unused operand: {other:?}"),
    }
}

#[test]
fn a_conditional_expression_only_runs_the_taken_branch() {
    let program = json!([{
        "$return": { "$if": true, "then": "taken", "else": { "$to_string": [] } }
    }]);
    let parsed = parse_program(&program).expect("parses");
    let module = InvocationModule::new(assemble_program(&parsed, &catalog()).expect("assembles"));
    let catalog = catalog();
    let context = json!({});
    match crate::vm::start(&module, &crate::vm::VmEnv::pure(&context, &catalog)) {
        InvocationStep::Complete { value } => assert_eq!(value, Value::from("taken")),
        other => panic!("the untaken branch was evaluated: {other:?}"),
    }
}

#[test]
fn an_if_statement_picks_its_branch() {
    let taken = json!([
        { "$if": { "value": true }, "then": [{ "$return": "yes" }], "else": [{ "$return": "no" }] }
    ]);
    assert_eq!(agree(&taken, &json!({})), Value::from("yes"));

    let untaken = json!([
        { "$if": { "value": false }, "then": [{ "$return": "yes" }], "else": [{ "$return": "no" }] }
    ]);
    assert_eq!(agree(&untaken, &json!({})), Value::from("no"));
}

#[test]
fn a_comparison_condition_agrees() {
    let program = json!([
        {
            "$if": { "value": { "$ref": { "input": ["n"] } }, "greater_than": 5 },
            "then": [{ "$return": "big" }],
            "else": [{ "$return": "small" }]
        }
    ]);
    assert_eq!(
        agree(&program, &json!({ "input": { "n": 9 } })),
        Value::from("big")
    );
    assert_eq!(
        agree(&program, &json!({ "input": { "n": 1 } })),
        Value::from("small")
    );
}

#[test]
fn an_all_junction_short_circuits_on_the_first_false() {
    let program = json!([
        {
            "$if": { "all": [{ "value": false }, { "value": true }] },
            "then": [{ "$return": "yes" }],
            "else": [{ "$return": "no" }]
        }
    ]);
    assert_eq!(agree(&program, &json!({})), Value::from("no"));
}

#[test]
fn an_any_junction_short_circuits_on_the_first_true() {
    let program = json!([
        {
            "$if": { "any": [{ "value": true }, { "value": false }] },
            "then": [{ "$return": "yes" }],
            "else": [{ "$return": "no" }]
        }
    ]);
    assert_eq!(agree(&program, &json!({})), Value::from("yes"));
}

#[test]
fn a_negated_condition_agrees() {
    let program = json!([
        {
            "$if": { "not": { "value": false } },
            "then": [{ "$return": "yes" }],
            "else": [{ "$return": "no" }]
        }
    ]);
    assert_eq!(agree(&program, &json!({})), Value::from("yes"));
}

#[test]
fn goto_is_reported_rather_than_executed() {
    let parsed = parse_program(&json!([{ "$goto": "cleanup" }])).expect("parses");
    let module = InvocationModule::new(assemble_program(&parsed, &catalog()).expect("assembles"));
    let catalog = catalog();
    let context = json!({});
    match crate::vm::start(&module, &crate::vm::VmEnv::pure(&context, &catalog)) {
        InvocationStep::Goto { target } => assert_eq!(target, "cleanup"),
        other => panic!("expected a goto step, got {other:?}"),
    }
}

#[test]
fn a_bare_expression_statement_is_discarded() {
    // the evaluator's fallthrough is null, not the last value; the vm must agree or a program's
    // result would depend on which one ran it.
    let program = json!([{ "$call": "add", "args": [1, 2] }]);
    assert_eq!(agree(&program, &json!({})), Value::Null);
}

#[test]
fn a_provider_call_yields_rather_than_folding() {
    let mut catalog = CallableCatalog::builtin();
    catalog.add_provider(&runinator_models::providers::ProviderMetadata {
        name: "http".into(),
        actions: vec![runinator_models::providers::ActionMetadata::new(
            "get",
            "fetch a url",
        )],
        metadata: Default::default(),
    });
    let parsed = parse_program(&json!([{ "$return": { "$call": "http.get", "args": [] } }]))
        .expect("parses");
    let module = InvocationModule::new(assemble_program(&parsed, &catalog).expect("assembles"));
    let context = json!({});
    match crate::vm::start(&module, &crate::vm::VmEnv::pure(&context, &catalog)) {
        InvocationStep::Yield { effect, .. } => {
            assert_eq!(effect.target.display_name(), "http.get");
        }
        other => panic!("a provider call must yield, got {other:?}"),
    }
}

#[test]
fn a_module_function_is_called_by_name() {
    let entry =
        parse_program(&json!([{ "$return": { "$call": "double", "args": [4] } }])).expect("parses");
    let body = parse_program(&json!([{
        "$return": { "$call": "mul", "args": [{ "$ref": { "let": ["n"] } }, 2] }
    }]))
    .expect("parses");
    let module = assemble_module(
        &entry,
        &[("double".to_string(), vec!["n".to_string()], body, None)],
        &catalog(),
    )
    .expect("assembles");
    let catalog = catalog();
    let context = json!({});
    match crate::vm::start(&module, &crate::vm::VmEnv::pure(&context, &catalog)) {
        InvocationStep::Complete { value } => assert_eq!(value, Value::from(8)),
        other => panic!("expected 8, got {other:?}"),
    }
}
