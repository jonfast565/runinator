//! type inference and checking over workflow expressions: path traversal, intrinsic return types, and
//! the higher-order combinators whose result type depends on their lambda.

use super::*;

fn typed_provider() -> ProviderMetadata {
    ProviderMetadata {
        name: "typed".into(),
        actions: vec![
            ActionMetadata::new("make", "make typed output")
                .with_parameters(vec![ParameterMetadata::required(
                    "name",
                    RuninatorType::String,
                )])
                .with_results(vec![
                    ResultMetadata::new("count", RuninatorType::Integer),
                    ResultMetadata::new("payload", RuninatorType::Any),
                    ResultMetadata::new(
                        "items",
                        RuninatorType::array(RuninatorType::structure([(
                            "key",
                            RuninatorType::String,
                        )])),
                    ),
                ]),
        ],
        metadata: ProviderRuntimeMetadata::default(),
    }
}

fn typed_workflow(
    input_type: RuninatorType,
    node: runinator_models::value::Value,
) -> WorkflowDefinition {
    let mut wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "make" } } },
            {
                "id": "make",
                "kind": "action",
                "action": {
                    "provider": "typed",
                    "function": "make",
                    "configuration": { "name": { "$ref": { "params": ["name"] } } }
                },
                "transitions": { "on_success": { "$node": "checked" } }
            },
            node,
            { "id": "done", "kind": "end" }
        ]
    }));
    wf.input_type = input_type;
    wf
}

fn schema_type(schema: runinator_models::value::Value) -> RuninatorType {
    RuninatorType::from_json_schema(&schema)
}

#[test]
fn typed_validation_requires_known_input_paths() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": { "name": { "$ref": { "params": ["missing"] } } },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_rejects_implicit_concat_coercion() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": {
                "name": {
                    "$concat": [
                        "count:",
                        { "$ref": { "node": "make", "output": ["count"] } }
                    ]
                }
            },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_accepts_explicit_string_conversions() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": {
                "name": {
                    "$concat": [
                        "count:",
                        { "$to_string": { "$ref": { "node": "make", "output": ["count"] } } },
                        " json:",
                        { "$to_json_string": { "$ref": { "node": "make", "output": ["items"] } } }
                    ]
                }
            },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    validate_workflow_with_providers(&wf, &[typed_provider()])
        .expect("explicit conversions validate");
}

#[test]
fn typed_validation_allows_string_conversion_from_any() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "action",
            "action": {
                "provider": "check",
                "function": "check",
                "configuration": {
                    "config": {
                        "$to_string": { "$ref": { "node": "make", "output": ["payload"] } }
                    }
                }
            },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    let mut provider = check_provider(RuninatorType::String);
    provider.name = "check".into();
    validate_workflow_with_providers(&wf, &[typed_provider(), provider])
        .expect("any values may be converted to strings at runtime");
}

#[test]
fn typed_validation_uses_intrinsic_return_type_in_conditions() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "condition": {
                "value": {
                    "$call": "len",
                    "args": [
                        { "$ref": { "node": "make", "output": ["items"] } }
                    ]
                },
                "greater_than": 0
            },
            "parameters": { "name": "done" },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    validate_workflow_with_providers(&wf, &[typed_provider()])
        .expect("intrinsic result type should validate against condition operand");
}

#[test]
fn typed_validation_rejects_opaque_json_traversal() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": {
                "name": { "$ref": { "node": "make", "output": ["payload", "key"] } }
            },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_checks_action_parameter_types() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "integer" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "config",
            "parameters": { "name": "done" },
            "transitions": { "next": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

#[test]
fn typed_validation_infers_higher_order_map_result_type() {
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "$call": "map",
            "args": [
                { "$ref": { "params": ["users"] } },
                {
                    "$lambda": {
                        "params": ["u"],
                        "body": { "$ref": { "let": ["u", "id"] } }
                    }
                }
            ]
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "users",
        RuninatorField::required(RuninatorType::array(RuninatorType::typed_structure([(
            "id",
            RuninatorField::required(RuninatorType::String),
        )]))),
    )]);

    let provider = check_provider(RuninatorType::array(RuninatorType::String));
    validate_workflow_with_providers(&wf, &[provider])
        .expect("map result should resolve to string[]");
}

#[test]
fn typed_validation_rejects_higher_order_map_result_mismatch() {
    let mut wf = action_workflow(runinator_models::json!({
        "config": {
            "$call": "map",
            "args": [
                { "$ref": { "params": ["users"] } },
                {
                    "$lambda": {
                        "params": ["u"],
                        "body": { "$ref": { "let": ["u", "id"] } }
                    }
                }
            ]
        }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "users",
        RuninatorField::required(RuninatorType::array(RuninatorType::typed_structure([(
            "id",
            RuninatorField::required(RuninatorType::String),
        )]))),
    )]);

    let provider = check_provider(RuninatorType::array(RuninatorType::Integer));
    assert!(validate_workflow_with_providers(&wf, &[provider]).is_err());
}

#[test]
fn typed_validation_infers_first_element_type() {
    let mut wf = action_workflow(runinator_models::json!({
        "config": { "$call": "first", "args": [ { "$ref": { "params": ["items"] } } ] }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "items",
        RuninatorField::required(RuninatorType::array(RuninatorType::String)),
    )]);

    validate_workflow_with_providers(&wf, &[check_provider(RuninatorType::String)])
        .expect("first of string[] should resolve to string");
    assert!(
        validate_workflow_with_providers(&wf, &[check_provider(RuninatorType::Integer)]).is_err(),
        "first of string[] should not satisfy an integer parameter"
    );
}

#[test]
fn typed_validation_infers_sort_preserves_element_type() {
    let mut wf = action_workflow(runinator_models::json!({
        "config": { "$call": "sort", "args": [ { "$ref": { "params": ["items"] } } ] }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "items",
        RuninatorField::required(RuninatorType::array(RuninatorType::String)),
    )]);

    validate_workflow_with_providers(
        &wf,
        &[check_provider(RuninatorType::array(RuninatorType::String))],
    )
    .expect("sort should preserve string[]");
    assert!(
        validate_workflow_with_providers(
            &wf,
            &[check_provider(RuninatorType::array(RuninatorType::Integer))]
        )
        .is_err(),
        "sort of string[] should not satisfy an integer[] parameter"
    );
}

#[test]
fn typed_validation_infers_values_of_map() {
    let mut wf = action_workflow(runinator_models::json!({
        "config": { "$call": "values", "args": [ { "$ref": { "params": ["scores"] } } ] }
    }));
    wf.input_type = RuninatorType::typed_structure([(
        "scores",
        RuninatorField::required(RuninatorType::map(RuninatorType::Integer)),
    )]);

    validate_workflow_with_providers(
        &wf,
        &[check_provider(RuninatorType::array(RuninatorType::Integer))],
    )
    .expect("values of map<integer> should resolve to integer[]");
}

#[test]
fn typed_validation_navigates_union_common_field() {
    let mut wf = action_workflow(runinator_models::json!({
        "config": { "$ref": { "params": ["u", "a"] } }
    }));
    // `u` is a union of two structs that both carry `a: integer`.
    wf.input_type = RuninatorType::typed_structure([(
        "u",
        RuninatorField::required(RuninatorType::Union(vec![
            RuninatorType::typed_structure([
                ("a", RuninatorField::required(RuninatorType::Integer)),
                ("b", RuninatorField::required(RuninatorType::String)),
            ]),
            RuninatorType::typed_structure([
                ("a", RuninatorField::required(RuninatorType::Integer)),
                ("c", RuninatorField::required(RuninatorType::Boolean)),
            ]),
        ])),
    )]);

    // `u.a` is integer in every variant, so it satisfies an integer parameter but not a string one.
    validate_workflow_with_providers(&wf, &[check_provider(RuninatorType::Integer)])
        .expect("union common field a resolves to integer");
    assert!(
        validate_workflow_with_providers(&wf, &[check_provider(RuninatorType::String)]).is_err(),
        "union common field a is integer, not string"
    );
}

#[test]
fn typed_validation_requires_map_items_to_be_array() {
    let mut wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
            {
                "id": "map",
                "kind": "map",
                "parameters": {
                    "items": { "$ref": { "params": ["name"] } },
                    "target": { "$node": "done" }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));
    wf.input_type = RuninatorType::from_json_schema(&runinator_models::json!({
        "type": "object",
        "properties": { "name": { "type": "string" } }
    }));

    assert!(validate_workflow_with_providers(&wf, &[]).is_err());
}

// the loop's declared output used to be a closed `{item, index}` while the reducer emitted five
// fields, so the three that answer "where am i" were written on every visit and rejected by every
// reader as a missing reference.
#[test]
fn typed_validation_exposes_the_whole_loop_output() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
            {
                "id": "each",
                "kind": "loop",
                "parameters": { "items": ["a", "b"] },
                "transitions": { "next": { "$node": "body" }, "on_success": { "$node": "done" } }
            },
            {
                "id": "body",
                "kind": "config",
                "parameters": {
                    "name": { "$ref": { "node": "each", "output": ["item"] } },
                    "metadata": {
                        "index": { "$ref": { "node": "each", "output": ["index"] } },
                        "count": { "$ref": { "node": "each", "output": ["count"] } },
                        "has_next": { "$ref": { "node": "each", "output": ["has_next"] } },
                        "last": { "$ref": { "node": "each", "output": ["last"] } }
                    }
                },
                "transitions": { "on_success": { "$node": "each" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow_with_providers(&wf, &[])
        .expect("every LoopOutput field must be referenceable");
}

// `runinator-rexrap-sema`'s `check_iterable` widens an unknown collection to unknown elements, but the
// graph checker used a bare `matches!(Array(_))` and rejected `any`. a workflow whose items come
// from an untyped upstream output therefore compiled clean and failed at import.
#[test]
fn typed_validation_accepts_loop_items_of_unknown_type() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "loop",
            // `payload` is declared `any` by the provider.
            "parameters": { "items": { "$ref": { "node": "make", "output": ["payload"] } } },
            "transitions": { "next": { "$node": "done" }, "on_success": { "$node": "done" } }
        }),
    );

    validate_workflow_with_providers(&wf, &[typed_provider()])
        .expect("an untyped collection must widen to untyped elements, not error");
}

// the widening must not become "accept anything": a concretely non-iterable source is still wrong.
#[test]
fn typed_validation_still_rejects_scalar_loop_items() {
    let wf = typed_workflow(
        schema_type(runinator_models::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } }
        })),
        runinator_models::json!({
            "id": "checked",
            "kind": "loop",
            "parameters": { "items": { "$ref": { "node": "make", "output": ["count"] } } },
            "transitions": { "next": { "$node": "done" }, "on_success": { "$node": "done" } }
        }),
    );

    assert!(validate_workflow_with_providers(&wf, &[typed_provider()]).is_err());
}

// a loop with no `items` at all is caught by the node kind's own parameter check, so authoring
// tools see it without running the whole type pass.
#[test]
fn a_loop_without_items_is_rejected() {
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
            {
                "id": "each",
                "kind": "loop",
                "transitions": { "next": { "$node": "done" }, "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    assert!(validate_workflow_with_providers(&wf, &[]).is_err());
}

#[test]
fn an_assert_block_may_compare_different_types_in_one_node() {
    // `assert` declares `assertions` as `Array(Any)`, and each entry's lowered condition is shaped
    // by whatever that assertion compares -- a string in one, an integer in the next. Inferring
    // the array by requiring one common item type contradicted the kind's own declared shape and
    // made any mixed-comparison assert block unrepresentable. This is the exact node REXRAP lowers
    // `assert { "a": x == y, "b": n == 0 }` into.
    let provider = ProviderMetadata {
        name: "checks".into(),
        actions: vec![
            ActionMetadata::new("summary", "check summary").with_results(vec![
                ResultMetadata::new("revision", RuninatorType::String),
                ResultMetadata::new("status", RuninatorType::String),
                ResultMetadata::new("failed", RuninatorType::Integer),
            ]),
        ],
        metadata: ProviderRuntimeMetadata::default(),
    };
    let wf = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "checks" } } },
            {
                "id": "checks",
                "kind": "action",
                "action": { "provider": "checks", "function": "summary", "configuration": {} },
                "transitions": { "on_success": { "$node": "verify" } }
            },
            {
                "id": "verify",
                "kind": "assert",
                "parameters": {
                    "assertions": [
                        {
                            "name": "checks_complete",
                            "message": "checks_complete",
                            "condition": {
                                "value": { "$ref": { "node": "checks", "output": ["status"] } },
                                "equals": "completed"
                            }
                        },
                        {
                            "name": "checks_passed",
                            "message": "checks_passed",
                            "condition": {
                                "value": { "$ref": { "node": "checks", "output": ["failed"] } },
                                "equals": 0
                            }
                        }
                    ]
                },
                "transitions": {
                    "on_success": { "$node": "done" },
                    "on_failure": { "$node": "done" }
                }
            },
            { "id": "done", "kind": "end" }
        ]
    }));

    validate_workflow_with_providers(&wf, &[provider])
        .expect("assertions comparing a string and an integer validate together");
}

#[test]
fn a_heterogeneous_array_still_fails_against_a_typed_parameter() {
    // widening must not silently accept a real mismatch: the union is reported where the value is
    // assigned to a declared type, which names what was expected instead of the shapeless
    // "incompatible item types".
    let wf = typed_workflow(
        RuninatorType::Any,
        runinator_models::json!({
            "id": "checked",
            "kind": "action",
            "action": {
                "provider": "typed",
                "function": "make",
                "configuration": { "name": [1, "two"] }
            },
            "transitions": { "on_success": { "$node": "done" } }
        }),
    );

    let error = validate_workflow_with_providers(&wf, &[typed_provider()])
        .expect_err("an array is not a string parameter");
    assert!(
        error.to_string().contains("name"),
        "the error should name the parameter that did not accept it, got: {error}"
    );
}
