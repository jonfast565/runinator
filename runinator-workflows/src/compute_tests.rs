use runinator_models::json;
use runinator_models::value::Value;

use crate::compute::{ComputeOutcome, PureIntrinsics, parse_program, run_program};
use crate::expressions::resolve_value_refs;

#[test]
fn arithmetic_preserves_integers() {
    let value = json!({ "$add": [2, 3] });
    assert_eq!(resolve_value_refs(&value, &Value::Null).unwrap(), json!(5));
}

#[test]
fn arithmetic_promotes_to_float() {
    let value = json!({ "$mul": [2, 2.5] });
    assert_eq!(
        resolve_value_refs(&value, &Value::Null).unwrap(),
        json!(5.0)
    );
}

#[test]
fn division_by_zero_errors() {
    let value = json!({ "$div": [1, 0] });
    assert!(resolve_value_refs(&value, &Value::Null).is_err());
}

#[test]
fn call_is_rejected_without_library() {
    let value = json!({ "$call": "add", "args": [1, 2] });
    assert!(resolve_value_refs(&value, &Value::Null).is_err());
}

#[test]
fn pure_program_returns_value() {
    let program = parse_program(&json!([
        { "$let": "total", "value": { "$add": [{ "$ref": { "input": ["a"] } }, 3] } },
        { "$return": { "total": { "$ref": { "let": ["total"] } } } }
    ]))
    .unwrap();
    let context = json!({ "input": { "a": 4 } });
    let outcome = run_program(&program, &context, &PureIntrinsics).unwrap();
    assert_eq!(outcome, ComputeOutcome::Return(json!({ "total": 7 })));
}

#[test]
fn goto_short_circuits() {
    let program = parse_program(&json!([
        { "$if": { "value": { "$ref": { "input": ["x"] } }, "less_than": 0 },
          "then": [ { "$goto": "fail" } ],
          "else": [] },
        { "$return": "ok" }
    ]))
    .unwrap();
    let negative =
        run_program(&program, &json!({ "input": { "x": -1 } }), &PureIntrinsics).unwrap();
    assert_eq!(negative, ComputeOutcome::Goto("fail".into()));
    let positive = run_program(&program, &json!({ "input": { "x": 1 } }), &PureIntrinsics).unwrap();
    assert_eq!(positive, ComputeOutcome::Return(json!("ok")));
}

#[test]
fn condition_resolves_call_via_library() {
    // a compute `if` whose condition calls an intrinsic resolves it through the library.
    let program = parse_program(&json!([
        { "$if": { "value": { "$call": "len", "args": [{ "$ref": { "input": ["xs"] } }] }, "greater_than": 1 },
          "then": [ { "$return": "many" } ],
          "else": [ { "$return": "few" } ] }
    ]))
    .unwrap();
    let many = run_program(
        &program,
        &json!({ "input": { "xs": [1, 2, 3] } }),
        &PureIntrinsics,
    )
    .unwrap();
    assert_eq!(many, ComputeOutcome::Return(json!("many")));
    let few = run_program(
        &program,
        &json!({ "input": { "xs": [1] } }),
        &PureIntrinsics,
    )
    .unwrap();
    assert_eq!(few, ComputeOutcome::Return(json!("few")));
}

#[test]
fn intrinsic_call_via_library() {
    let program = parse_program(&json!([
        { "$return": { "$call": "upper", "args": ["hello"] } }
    ]))
    .unwrap();
    let outcome = run_program(&program, &Value::Null, &PureIntrinsics).unwrap();
    assert_eq!(outcome, ComputeOutcome::Return(json!("HELLO")));
}

#[test]
fn unknown_intrinsic_rejected() {
    let program = parse_program(&json!([
        { "$return": { "$call": "nope", "args": [] } }
    ]))
    .unwrap();
    assert!(run_program(&program, &Value::Null, &PureIntrinsics).is_err());
}

#[test]
fn len_and_keys_intrinsics() {
    assert_eq!(
        PureIntrinsics.call_for_test("len", &[json!([1, 2, 3])]),
        json!(3)
    );
    assert_eq!(
        PureIntrinsics.call_for_test("keys", &[json!({ "b": 1, "a": 2 })]),
        json!(["a", "b"])
    );
}

#[test]
fn signatures_cover_every_name() {
    let names: Vec<&str> = PureIntrinsics::signatures()
        .iter()
        .map(|action| action.function_name.clone())
        .map(|name| Box::leak(name.into_boxed_str()) as &str)
        .collect();
    for name in PureIntrinsics::names() {
        assert!(names.contains(name), "missing signature for {name}");
    }
}

// small test helper to invoke an intrinsic and unwrap.
impl PureIntrinsics {
    fn call_for_test(&self, name: &str, args: &[Value]) -> Value {
        use crate::compute::IntrinsicLibrary;
        self.call(name, args).unwrap()
    }
}
