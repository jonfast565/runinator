use runinator_models::json;
use runinator_models::value::Value;

use crate::{
    EFFECTFUL_INTRINSIC_NAMES, FunctionTable, HIGHER_ORDER_NAMES, PureIntrinsics,
    intrinsic_catalog, is_known_intrinsic, resolve_value_refs_with_functions,
};

// the generated catalog is the front end's view of the callable intrinsics; it must cover exactly
// the names the runtime knows so the two cannot drift.
#[test]
fn catalog_covers_every_known_intrinsic() {
    let catalog = intrinsic_catalog();
    let names: Vec<&str> = catalog
        .iter()
        .map(|action| action.function_name.as_str())
        .collect();
    for expected in PureIntrinsics::names()
        .iter()
        .chain(EFFECTFUL_INTRINSIC_NAMES)
        .chain(HIGHER_ORDER_NAMES)
    {
        assert!(names.contains(expected), "catalog missing '{expected}'");
    }
    for name in &names {
        assert!(is_known_intrinsic(name), "catalog has unknown '{name}'");
    }
}

fn table(value: Value) -> FunctionTable {
    FunctionTable::from_metadata(Some(&value)).expect("parse functions")
}

fn local_ref(name: &str) -> Value {
    json!({ "$ref": { "let": [name] } })
}

// double(x) = x * 2, called as double(21) -> 42.
#[test]
fn evaluates_a_user_function_call() {
    let functions = table(json!([
        {
            "name": "double",
            "params": [{ "name": "x" }],
            "body": { "$mul": [local_ref("x"), 2] }
        }
    ]));
    let call = json!({ "$call": "double", "args": [21] });
    let result = resolve_value_refs_with_functions(&call, &json!({}), &functions).expect("eval");
    assert_eq!(result, Value::from(42));
}

// nested user calls share no recursion: double(inc(5)) -> 12.
#[test]
fn evaluates_nested_user_functions() {
    let functions = table(json!([
        { "name": "inc", "params": ["x"], "body": { "$call": "add", "args": [local_ref("x"), 1] } },
        { "name": "double", "params": ["x"], "body": { "$call": "add", "args": [local_ref("x"), local_ref("x")] } }
    ]));
    let call = json!({ "$call": "double", "args": [{ "$call": "inc", "args": [5] }] });
    let result = resolve_value_refs_with_functions(&call, &json!({}), &functions).expect("eval");
    assert_eq!(result, Value::from(12));
}

// a recursive factorial terminates via its base case: the conditional is lazy, so the recursive
// branch is not evaluated once n <= 1. fact(5) -> 120, well under the depth cap.
#[test]
fn evaluates_recursive_factorial() {
    let functions = table(json!([
        {
            "name": "fact",
            "params": ["n"],
            "recursive": { "max_depth": 50 },
            "body": {
                "$if": { "$call": "lte", "args": [local_ref("n"), 1] },
                "then": 1,
                "else": {
                    "$mul": [
                        local_ref("n"),
                        { "$call": "fact", "args": [{ "$sub": [local_ref("n"), 1] }] }
                    ]
                }
            }
        }
    ]));
    let call = json!({ "$call": "fact", "args": [5] });
    let result = resolve_value_refs_with_functions(&call, &json!({}), &functions).expect("eval");
    assert_eq!(result, Value::from(120));
}

// a block (program) body threads local `let` bindings and returns the final value: build(2, 3)
// binds sum = add(a, b) then returns sum -> 5.
#[test]
fn evaluates_a_program_body_function() {
    let functions = table(json!([
        {
            "name": "build",
            "params": ["a", "b"],
            "program": [
                { "$let": "sum", "value": { "$call": "add", "args": [local_ref("a"), local_ref("b")] } },
                { "$return": local_ref("sum") }
            ]
        }
    ]));
    let call = json!({ "$call": "build", "args": [2, 3] });
    let result = resolve_value_refs_with_functions(&call, &json!({}), &functions).expect("eval");
    assert_eq!(result, Value::from(5));
}

// a program body that never returns is a void function and yields null.
#[test]
fn program_body_without_return_is_void() {
    let functions = table(json!([
        {
            "name": "noop",
            "params": ["x"],
            "program": [
                { "$let": "y", "value": local_ref("x") }
            ]
        }
    ]));
    let call = json!({ "$call": "noop", "args": [7] });
    let result = resolve_value_refs_with_functions(&call, &json!({}), &functions).expect("eval");
    assert_eq!(result, Value::Null);
}

// the recursion limit still applies when the body is a program rather than an expression.
#[test]
fn program_body_recursion_fails_past_max_depth() {
    let functions = table(json!([
        {
            "name": "loopy",
            "params": ["n"],
            "recursive": { "max_depth": 3 },
            "program": [
                { "$return": { "$call": "loopy", "args": [local_ref("n")] } }
            ]
        }
    ]));
    let call = json!({ "$call": "loopy", "args": [0] });
    let err = resolve_value_refs_with_functions(&call, &json!({}), &functions)
        .expect_err("should exceed recursion limit");
    assert!(
        err.to_string().contains("recursion limit"),
        "unexpected error: {err}"
    );
}

// a recursive function fails once it exceeds its declared max_depth.
#[test]
fn recursion_fails_past_max_depth() {
    let functions = table(json!([
        {
            "name": "loopy",
            "params": ["n"],
            "body": { "$call": "loopy", "args": [local_ref("n")] },
            "recursive": { "max_depth": 3 }
        }
    ]));
    let call = json!({ "$call": "loopy", "args": [0] });
    let err = resolve_value_refs_with_functions(&call, &json!({}), &functions)
        .expect_err("should exceed recursion limit");
    assert!(
        err.to_string().contains("recursion limit"),
        "unexpected error: {err}"
    );
}
