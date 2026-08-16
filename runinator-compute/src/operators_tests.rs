//! the operator intrinsics, checked against the evaluator arms they were transcribed from.
//!
//! each case here is one where the two could plausibly drift: integer versus float arithmetic, what
//! `string()` refuses, what counts as truthy. the evaluator is driven through a literal expression
//! so the comparison is against the real arm rather than against a restatement of it.

use super::*;

use crate::expressions::evaluate_expression;
use runinator_models::json;
use runinator_models::workflow_ast::WorkflowExpression;

/// evaluate `expr` on the tree evaluator with an empty context.
fn evaluated(expr: &Value) -> Result<Value, WorkflowValidationError> {
    let parsed = WorkflowExpression::try_from(expr.clone())
        .map_err(|err| WorkflowValidationError::InvalidValueRef(format!("{err:?}")))?;
    evaluate_expression(&parsed, &json!({}))
}

#[test]
fn concat_matches_the_evaluator() {
    let args = [Value::from("a"), Value::from("b")];
    assert_eq!(
        call_operator(crate::assemble::CONCAT_INTRINSIC, &args).unwrap(),
        evaluated(&json!({ "$concat": ["a", "b"] })).unwrap()
    );
}

#[test]
fn concat_rejects_a_non_string_in_both() {
    let args = [Value::from("a"), Value::from(1)];
    assert!(call_operator(crate::assemble::CONCAT_INTRINSIC, &args).is_err());
    assert!(evaluated(&json!({ "$concat": ["a", 1] })).is_err());
}

#[test]
fn to_string_matches_the_evaluator_for_each_accepted_type() {
    for value in [Value::from("s"), Value::from(true), Value::from(3)] {
        assert_eq!(
            call_operator(crate::assemble::TO_STRING_INTRINSIC, &[value.clone()]).unwrap(),
            evaluated(&json!({ "$to_string": value })).unwrap(),
            "disagreed about string({value})"
        );
    }
}

#[test]
fn to_string_refuses_the_same_types_the_evaluator_does() {
    for value in [Value::Null, json!([1]), json!({ "a": 1 })] {
        assert!(call_operator(crate::assemble::TO_STRING_INTRINSIC, &[value.clone()]).is_err());
        assert!(evaluated(&json!({ "$to_string": value })).is_err());
    }
}

#[test]
fn to_json_matches_the_evaluator() {
    let value = json!({ "b": 1, "a": 2 });
    assert_eq!(
        call_operator(crate::assemble::TO_JSON_INTRINSIC, &[value.clone()]).unwrap(),
        evaluated(&json!({ "$to_json_string": value })).unwrap()
    );
}

#[test]
fn neg_wraps_integers_and_negates_floats_like_the_evaluator() {
    for value in [Value::from(5), Value::from(-5), Value::from(2.5)] {
        assert_eq!(
            call_operator(crate::assemble::NEG_INTRINSIC, &[value.clone()]).unwrap(),
            evaluated(&json!({ "$neg": value })).unwrap(),
            "disagreed about -({value})"
        );
    }
}

#[test]
fn neg_keeps_an_integer_an_integer() {
    // the case that would silently widen: `-3` must not become `-3.0`, or a downstream `at(xs, -i)`
    // would stop being an index.
    let negated = call_operator(crate::assemble::NEG_INTRINSIC, &[Value::from(3)]).unwrap();
    assert_eq!(negated, Value::from(-3));
    assert!(negated.as_i64().is_some(), "-3 must stay an integer");
}

#[test]
fn truthiness_is_the_vms_single_rule() {
    // only null and false are falsy — notably 0 and "" are truthy, which is where the evaluator's
    // three former rules disagreed with each other.
    for (value, expected) in [
        (Value::Null, false),
        (Value::from(false), false),
        (Value::from(true), true),
        (Value::from(0), true),
        (Value::from(""), true),
        (json!([]), true),
    ] {
        assert_eq!(
            call_operator(crate::assemble::TRUTHY_INTRINSIC, &[value.clone()]).unwrap(),
            Value::from(expected),
            "disagreed about the truthiness of {value}"
        );
    }
}

#[test]
fn not_inverts_truthiness() {
    assert_eq!(
        call_operator(crate::assemble::NOT_INTRINSIC, &[Value::Null]).unwrap(),
        Value::from(true)
    );
    assert_eq!(
        call_operator(crate::assemble::NOT_INTRINSIC, &[Value::from(1)]).unwrap(),
        Value::from(false)
    );
}

#[test]
fn exists_tests_presence_not_truthiness() {
    // `false` exists; only null does not. conflating the two is the bug this separation prevents.
    assert_eq!(
        call_operator(crate::assemble::EXISTS_INTRINSIC, &[Value::from(false)]).unwrap(),
        Value::from(true)
    );
    assert_eq!(
        call_operator(crate::assemble::EXISTS_INTRINSIC, &[Value::Null]).unwrap(),
        Value::from(false)
    );
}

#[test]
fn is_null_is_the_complement_of_exists() {
    for value in [Value::Null, Value::from(false), Value::from(0)] {
        let is_null = call_operator(crate::assemble::IS_NULL_INTRINSIC, &[value.clone()]).unwrap();
        let exists = call_operator(crate::assemble::EXISTS_INTRINSIC, &[value.clone()]).unwrap();
        assert_eq!(is_null, Value::from(!exists.as_bool().unwrap()));
    }
}

#[test]
fn in_swaps_its_operands_relative_to_contains() {
    // `"a" in ["a"]` is `contains(["a"], "a")`.
    let args = [Value::from("a"), json!(["a", "b"])];
    assert_eq!(
        call_operator(crate::assemble::IN_INTRINSIC, &args).unwrap(),
        Value::from(true)
    );
}

#[test]
fn an_unknown_name_is_not_an_operator() {
    assert!(!is_operator_intrinsic("add"));
    assert!(is_operator_intrinsic(crate::assemble::CONCAT_INTRINSIC));
    assert!(call_operator("add", &[]).is_err());
}
