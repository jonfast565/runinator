use runinator_models::json;
use runinator_models::value::Value;

use crate::compute::{
    ComputeOutcome, EFFECTFUL_INTRINSIC_NAMES, HIGHER_ORDER_NAMES, PureIntrinsics, STD_MODULES,
    intrinsic_module, parse_program, qualified_intrinsic_name, resolve_std_path, run_program,
};
use crate::expressions::resolve_value_refs;

#[test]
fn every_intrinsic_maps_to_a_known_std_module() {
    // the module map is the single source of truth for surface qualification; every pure,
    // effectful, and higher-order intrinsic must resolve to a declared std module, and the
    // qualified name must round-trip back to the same leaf.
    let leaves = PureIntrinsics::names()
        .iter()
        .chain(EFFECTFUL_INTRINSIC_NAMES.iter())
        .chain(HIGHER_ORDER_NAMES.iter());
    for leaf in leaves {
        let module = intrinsic_module(leaf)
            .unwrap_or_else(|| panic!("intrinsic '{leaf}' has no std module"));
        assert!(
            STD_MODULES.contains(&module),
            "intrinsic '{leaf}' maps to undeclared module '{module}'"
        );
        assert_eq!(
            qualified_intrinsic_name(leaf).as_deref(),
            Some(&*format!("std.{module}.{leaf}"))
        );
        assert_eq!(resolve_std_path(module, leaf), Ok(module));
    }
}

#[test]
fn resolve_std_path_rejects_wrong_module() {
    // `upper` lives in strings, so addressing it through math is a hard error that names the
    // correct module.
    assert_eq!(resolve_std_path("math", "upper"), Err(Some("strings")));
    assert_eq!(resolve_std_path("strings", "not_a_function"), Err(None));
}

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
fn pure_call_folds_eagerly() {
    // the eager reducer path carries the pure standard library, so a pure `$call` folds in place.
    let value = json!({ "$call": "add", "args": [1, 2] });
    assert_eq!(resolve_value_refs(&value, &Value::Null).unwrap(), json!(3));
}

#[test]
fn effectful_call_is_rejected_eagerly() {
    // an effectful intrinsic is not in the pure library, so the eager path errors.
    let value = json!({ "$call": "http_get", "args": ["http://example.test"] });
    assert!(resolve_value_refs(&value, &Value::Null).is_err());
}

#[test]
fn pure_program_returns_value() {
    let program = parse_program(&json!([
        { "$let": "total", "value": { "$add": [{ "$ref": { "params": ["a"] } }, 3] } },
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
        { "$if": { "value": { "$ref": { "params": ["x"] } }, "less_than": 0 },
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
        { "$if": { "value": { "$call": "len", "args": [{ "$ref": { "params": ["xs"] } }] }, "greater_than": 1 },
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

#[test]
fn every_name_has_arity() {
    for name in PureIntrinsics::names() {
        assert!(
            crate::compute::intrinsic_arity(name).is_some(),
            "missing arity for {name}"
        );
    }
}

#[test]
fn string_intrinsics() {
    assert_eq!(
        PureIntrinsics.call_for_test("split", &[json!("a,b,c"), json!(",")]),
        json!(["a", "b", "c"])
    );
    assert_eq!(
        PureIntrinsics.call_for_test("join", &[json!(["a", "b", "c"]), json!("-")]),
        json!("a-b-c")
    );
    assert_eq!(
        PureIntrinsics.call_for_test("replace", &[json!("a.b.a"), json!("a"), json!("x")]),
        json!("x.b.x")
    );
    assert_eq!(
        PureIntrinsics.call_for_test("substring", &[json!("hello"), json!(1), json!(3)]),
        json!("el")
    );
    assert_eq!(
        PureIntrinsics.call_for_test("trim", &[json!("  hi  ")]),
        json!("hi")
    );
}

#[test]
fn collection_intrinsics() {
    assert_eq!(
        PureIntrinsics.call_for_test("sort", &[json!([3, 1, 2])]),
        json!([1, 2, 3])
    );
    assert_eq!(
        PureIntrinsics.call_for_test("unique", &[json!([1, 1, 2, 1, 3])]),
        json!([1, 2, 3])
    );
    assert_eq!(
        PureIntrinsics.call_for_test("flatten", &[json!([[1, 2], [3], 4])]),
        json!([1, 2, 3, 4])
    );
    assert_eq!(
        PureIntrinsics.call_for_test("sum", &[json!([1, 2, 3])]),
        json!(6)
    );
    assert_eq!(
        PureIntrinsics.call_for_test("range", &[json!(0), json!(3)]),
        json!([0, 1, 2])
    );
    assert_eq!(
        PureIntrinsics.call_for_test("at", &[json!([10, 20, 30]), json!(-1)]),
        json!(30)
    );
    assert_eq!(
        PureIntrinsics.call_for_test("contains", &[json!([1, 2, 3]), json!(2)]),
        json!(true)
    );
}

#[test]
fn object_intrinsics() {
    assert_eq!(
        PureIntrinsics.call_for_test("merge", &[json!({ "a": 1 }), json!({ "b": 2, "a": 9 })]),
        json!({ "a": 9, "b": 2 })
    );
    assert_eq!(
        PureIntrinsics.call_for_test(
            "pick",
            &[json!({ "a": 1, "b": 2, "c": 3 }), json!(["a", "c"])]
        ),
        json!({ "a": 1, "c": 3 })
    );
    assert_eq!(
        PureIntrinsics.call_for_test("omit", &[json!({ "a": 1, "b": 2 }), json!(["a"])]),
        json!({ "b": 2 })
    );
    let entries = PureIntrinsics.call_for_test("entries", &[json!({ "a": 1 })]);
    assert_eq!(entries, json!([{ "key": "a", "value": 1 }]));
    assert_eq!(
        PureIntrinsics.call_for_test("from_entries", &[entries]),
        json!({ "a": 1 })
    );
}

#[test]
fn encoding_and_logic_intrinsics() {
    assert_eq!(
        PureIntrinsics.call_for_test("parse_json", &[json!("{\"a\":1}")]),
        json!({ "a": 1 })
    );
    let encoded = PureIntrinsics.call_for_test("base64_encode", &[json!("hi")]);
    assert_eq!(
        PureIntrinsics.call_for_test("base64_decode", &[encoded]),
        json!("hi")
    );
    assert_eq!(
        PureIntrinsics.call_for_test("gt", &[json!(5), json!(3)]),
        json!(true)
    );
    assert_eq!(
        PureIntrinsics.call_for_test("default", &[Value::Null, json!("fallback")]),
        json!("fallback")
    );
}

#[test]
fn date_intrinsics() {
    let shifted = PureIntrinsics.call_for_test(
        "add_duration",
        &[json!("2026-01-01T00:00:00+00:00"), json!(3600)],
    );
    assert_eq!(
        PureIntrinsics.call_for_test("date_diff", &[shifted, json!("2026-01-01T00:00:00+00:00")]),
        json!(3600)
    );
    assert_eq!(
        PureIntrinsics.call_for_test("format_date", &[json!(0), json!("%Y-%m-%d")]),
        json!("1970-01-01")
    );
}

#[test]
fn regex_intrinsics() {
    assert_eq!(
        PureIntrinsics.call_for_test("regex_match", &[json!("abc123"), json!("[0-9]+")]),
        json!(true)
    );
    assert_eq!(
        PureIntrinsics.call_for_test("regex_extract", &[json!("a1b2c3"), json!("[0-9]")]),
        json!(["1", "2", "3"])
    );
    assert_eq!(
        PureIntrinsics.call_for_test(
            "regex_replace",
            &[json!("a1b2"), json!("[0-9]"), json!("#")]
        ),
        json!("a#b#")
    );
}

#[test]
fn lambda_map_doubles_elements() {
    let program = parse_program(&json!([
        { "$return": { "$call": "map", "args": [
            { "$ref": { "params": ["xs"] } },
            { "$lambda": { "params": ["x"], "body": { "$mul": [{ "$ref": { "let": ["x"] } }, 2] } } }
        ] } }
    ]))
    .unwrap();
    let outcome = run_program(
        &program,
        &json!({ "input": { "xs": [1, 2, 3] } }),
        &PureIntrinsics,
    )
    .unwrap();
    assert_eq!(outcome, ComputeOutcome::Return(json!([2, 4, 6])));
}

#[test]
fn lambda_filter_keeps_matches() {
    let program = parse_program(&json!([
        { "$return": { "$call": "filter", "args": [
            { "$ref": { "params": ["xs"] } },
            { "$lambda": { "params": ["x"], "body": { "$call": "gt", "args": [{ "$ref": { "let": ["x"] } }, 1] } } }
        ] } }
    ]))
    .unwrap();
    let outcome = run_program(
        &program,
        &json!({ "input": { "xs": [0, 1, 2, 3] } }),
        &PureIntrinsics,
    )
    .unwrap();
    assert_eq!(outcome, ComputeOutcome::Return(json!([2, 3])));
}

#[test]
fn lambda_reduce_sums() {
    let program = parse_program(&json!([
        { "$return": { "$call": "reduce", "args": [
            { "$ref": { "params": ["xs"] } },
            0,
            { "$lambda": { "params": ["acc", "x"], "body": { "$add": [
                { "$ref": { "let": ["acc"] } }, { "$ref": { "let": ["x"] } }
            ] } } }
        ] } }
    ]))
    .unwrap();
    let outcome = run_program(
        &program,
        &json!({ "input": { "xs": [1, 2, 3, 4] } }),
        &PureIntrinsics,
    )
    .unwrap();
    assert_eq!(outcome, ComputeOutcome::Return(json!(10)));
}

#[test]
fn lambda_sort_by_key() {
    let program = parse_program(&json!([
        { "$return": { "$call": "sort_by", "args": [
            { "$ref": { "params": ["xs"] } },
            { "$lambda": { "params": ["u"], "body": { "$ref": { "let": ["u", "age"] } } } }
        ] } }
    ]))
    .unwrap();
    let context = json!({ "input": { "xs": [{ "age": 30 }, { "age": 10 }, { "age": 20 }] } });
    let outcome = run_program(&program, &context, &PureIntrinsics).unwrap();
    assert_eq!(
        outcome,
        ComputeOutcome::Return(json!([{ "age": 10 }, { "age": 20 }, { "age": 30 }]))
    );
}

#[test]
fn lambda_outside_higher_order_is_rejected() {
    let value = json!({ "$lambda": { "params": ["x"], "body": { "$ref": { "let": ["x"] } } } });
    assert!(resolve_value_refs(&value, &Value::Null).is_err());
}

#[test]
fn pure_resolver_evaluates_compute_tier() {
    use crate::expressions::resolve_value_refs_pure;
    // a pure `$call` resolves through both the default eager path and the explicit pure path.
    let call = json!({ "$call": "upper", "args": ["hi"] });
    assert_eq!(
        resolve_value_refs(&call, &Value::Null).unwrap(),
        json!("HI")
    );
    assert_eq!(
        resolve_value_refs_pure(&call, &Value::Null).unwrap(),
        json!("HI")
    );
    // a higher-order map with a lambda resolves against the context.
    let mapped = json!({ "$call": "map", "args": [
        { "$ref": { "params": ["xs"] } },
        { "$lambda": { "params": ["x"], "body": { "$mul": [{ "$ref": { "let": ["x"] } }, 10] } } }
    ] });
    assert_eq!(
        resolve_value_refs_pure(&mapped, &json!({ "input": { "xs": [1, 2] } })).unwrap(),
        json!([10, 20])
    );
    // effectful intrinsics are not available in a preview and error.
    let effectful = json!({ "$call": "http_get", "args": ["http://example.test"] });
    assert!(resolve_value_refs_pure(&effectful, &Value::Null).is_err());
}

// small test helper to invoke an intrinsic and unwrap.
impl PureIntrinsics {
    fn call_for_test(&self, name: &str, args: &[Value]) -> Value {
        use crate::compute::IntrinsicLibrary;
        self.call(name, args).unwrap()
    }
}
