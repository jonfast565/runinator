//! evaluating a standalone fragment — expression, condition, or compute — against its surface.

use super::*;

#[test]
fn validates_and_evaluates_expression_fragment() {
    let context = Value::from(serde_json::json!({ "input": { "name": "Ada" } }));
    let value = evaluate_fragment(
        r#""hello " ++ params.name"#,
        RexRapFragmentKind::Expression,
        &context,
        &CompileOptions::default(),
    )
    .expect("evaluate expression");

    assert_eq!(value, Value::from("hello Ada"));
}
#[test]
fn validates_and_evaluates_condition_fragment() {
    let context = Value::from(serde_json::json!({ "input": { "count": 3 } }));
    let value = evaluate_fragment(
        "params.count >= 3 && exists params.count",
        RexRapFragmentKind::Condition,
        &context,
        &CompileOptions::default(),
    )
    .expect("evaluate condition");

    assert_eq!(value, Value::from(true));
}
#[test]
fn validates_and_evaluates_compute_fragment() {
    let context = Value::from(serde_json::json!({ "input": { "count": 3 } }));
    let value = evaluate_fragment(
        r#"{ let doubled = params.count * 2 return doubled + 1 }"#,
        RexRapFragmentKind::Do,
        &context,
        &CompileOptions::default(),
    )
    .expect("evaluate compute");

    assert_eq!(value.get("outcome").and_then(Value::as_str), Some("return"));
    assert_eq!(value.get("value"), Some(&Value::from(7)));
}
#[test]
fn fragment_validation_rejects_wrong_surface() {
    let err = validate_fragment(
        "workflow \"Not a fragment\" {}",
        RexRapFragmentKind::Expression,
        &CompileOptions::default(),
    )
    .unwrap_err();

    assert!(err.to_string().contains("expected"), "{err}");
}
