// round-trip guard for the program ast relocated into `runinator_models::workflow_ast`. the field
// typing in later phases relies on `Value -> WorkflowExpression -> Value` being identity for the
// canonical forms the wdl lowerer emits, so these tests pin exactly that. non-canonical spellings
// (the `input` ref alias, an explicit `$literal`) intentionally normalize; a separate test documents
// that so the behavior change is deliberate, not accidental.

use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::workflow_ast::{ConditionNode, WorkflowExpression};

// the canonical expression forms the lowerer produces (see runinator-wdl/src/lower/expr.rs). every
// entry must satisfy `static(parse(v)) == v`.
fn canonical_expressions() -> Vec<Value> {
    vec![
        json!(null),
        json!(true),
        json!(42),
        json!(-7),
        json!(3.5),
        json!("hello"),
        json!({ "$ref": { "params": ["user", "name"] } }),
        json!({ "$ref": { "prev": [] } }),
        json!({ "$ref": { "workflow": ["run", "id"] } }),
        json!({ "$ref": { "config": ["endpoint"] } }),
        json!({ "$ref": { "let": ["x"] } }),
        json!({ "$ref": { "node": "fetch", "output": ["body", 0] } }),
        json!({ "$concat": [{ "$ref": { "params": ["name"] } }, " world"] }),
        json!({ "$coalesce": [{ "$ref": { "prev": [] } }, "default"] }),
        json!({ "$to_string": 5 }),
        json!({ "$to_json_string": { "a": 1 } }),
        json!({ "$add": [1, 2, 3] }),
        json!({ "$sub": [10, 4] }),
        json!({ "$mul": [2, 3] }),
        json!({ "$div": [12, 4] }),
        json!({ "$mod": [7, 3] }),
        json!({ "$neg": 5 }),
        json!({ "$call": "add", "args": [1, 2] }),
        json!({ "$if": { "$ref": { "params": ["ok"] } }, "then": 1, "else": 0 }),
        json!({
            "$call": "map",
            "args": [
                { "$ref": { "params": ["items"] } },
                { "$lambda": { "params": ["x"], "body": { "$ref": { "let": ["x"] } } } }
            ]
        }),
        // a literal object carrying a live ref: parses to Literal(Object) but the inner ref
        // round-trips through static evaluation, so the whole value is preserved.
        json!({ "a": 1, "b": { "$ref": { "config": ["k"] } } }),
        // a literal array.
        json!([1, "two", { "$ref": { "params": ["z"] } }]),
    ]
}

#[test]
fn canonical_expressions_round_trip() {
    for value in canonical_expressions() {
        let parsed = WorkflowExpression::try_from(&value)
            .unwrap_or_else(|err| panic!("parse failed for {value}: {err}"));
        let reserialized = Value::from(&parsed);
        assert_eq!(
            reserialized, value,
            "round-trip changed the value for {value}"
        );
    }
}

#[test]
fn parse_is_idempotent_via_typed_form() {
    // static(parse(v)) is canonical, so re-parsing it yields an equal typed form.
    for value in canonical_expressions() {
        let once = WorkflowExpression::try_from(&value).expect("first parse");
        let canonical = Value::from(&once);
        let twice = WorkflowExpression::try_from(&canonical).expect("second parse");
        assert_eq!(once, twice, "typed form not stable for {value}");
    }
}

#[test]
fn canonical_compute_program_round_trips() {
    // the `$let`/`$if`/`$return`/`$goto`/bare-expr forms lower/compute.rs emits; parsing then
    // serializing (the now-relocated ComputeProgram -> Value) must reproduce the program verbatim.
    let program = json!([
        { "$let": "x", "value": { "$ref": { "params": ["n"] } } },
        { "$if": { "value": { "$ref": { "let": ["x"] } }, "greater_than": 0 },
          "then": [ { "$return": "pos" } ],
          "else": [ { "$goto": "recover" } ] },
        { "$call": "add", "args": [1, 2] }
    ]);
    let parsed = crate::compute::parse_program(&program).expect("parse program");
    assert_eq!(
        Value::from(&parsed),
        program,
        "compute program round-trip changed"
    );
}

// the canonical condition forms the lowerer produces (see runinator-wdl/src/lower/expr.rs
// `lower_cond`). every entry must satisfy `Value::from(ConditionNode::from(v)) == v`.
fn canonical_conditions() -> Vec<Value> {
    vec![
        json!({ "value": { "$ref": { "params": ["flag"] } } }),
        json!({ "value": { "$ref": { "params": ["name"] } }, "exists": true }),
        json!({ "value": { "$ref": { "params": ["name"] } }, "exists": false }),
        json!({ "value": { "$ref": { "params": ["count"] } }, "equals": 3 }),
        json!({ "value": { "$ref": { "params": ["count"] } }, "not_equals": 0 }),
        json!({ "value": { "$ref": { "params": ["role"] } }, "contains": "admin" }),
        json!({ "value": { "$ref": { "params": ["role"] } }, "in": ["a", "b"] }),
        json!({ "value": { "$ref": { "params": ["name"] } }, "starts_with": "x" }),
        json!({ "value": { "$ref": { "params": ["name"] } }, "ends_with": "z" }),
        json!({ "value": { "$ref": { "params": ["n"] } }, "greater_than": 1 }),
        json!({ "value": { "$ref": { "params": ["n"] } }, "greater_than_or_equal": 1 }),
        json!({ "value": { "$ref": { "params": ["n"] } }, "less_than": 10 }),
        json!({ "value": { "$ref": { "params": ["n"] } }, "less_than_or_equal": 10 }),
        json!({ "all": [
            { "value": { "$ref": { "params": ["a"] } }, "equals": 1 },
            { "value": { "$ref": { "params": ["b"] } }, "equals": 2 }
        ] }),
        json!({ "any": [
            { "value": { "$ref": { "params": ["a"] } } },
            { "not": { "value": { "$ref": { "params": ["b"] } } } }
        ] }),
    ]
}

#[test]
fn canonical_conditions_round_trip() {
    for value in canonical_conditions() {
        let node = ConditionNode::from(&value);
        assert!(
            !matches!(node, ConditionNode::Other(_)),
            "canonical condition parsed as Other: {value}"
        );
        let reserialized = Value::from(&node);
        assert_eq!(reserialized, value, "round-trip changed condition {value}");
    }
}

#[test]
fn unknown_condition_is_preserved_verbatim() {
    // a shape the evaluator does not recognize is carried as Other and round-trips byte-identically.
    let odd = json!({ "left": { "$ref": { "params": ["a"] } } });
    let node = ConditionNode::from(&odd);
    assert!(matches!(node, ConditionNode::Other(_)));
    assert_eq!(Value::from(&node), odd);
}

#[test]
fn non_canonical_spellings_normalize() {
    // the `input` ref alias canonicalizes to `params`.
    let aliased = json!({ "$ref": { "input": ["a"] } });
    let canonical = Value::from(&WorkflowExpression::try_from(&aliased).unwrap());
    assert_eq!(canonical, json!({ "$ref": { "params": ["a"] } }));

    // an explicit `$literal` unwraps to the bare value.
    let wrapped = json!({ "$literal": 5 });
    let canonical = Value::from(&WorkflowExpression::try_from(&wrapped).unwrap());
    assert_eq!(canonical, json!(5));
}
