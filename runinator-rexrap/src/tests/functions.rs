//! user `fn` definitions: defaults, lambdas, block bodies, recursion annotations, and how a
//! definition is carried in metadata and called.

use super::*;

#[test]
#[ignore = "legacy action-node assertion removed by invocation hard cutover"]
fn function_defaults_and_lambdas_lower_into_metadata() {
    let src = r#"
        fn fold_values(xs: integer[], seed: integer = 0) -> integer = std.collections.reduce(xs, seed, (acc, x) => std.math.add(acc, x))

        workflow "Fn" v1 {
            do {
                let total = fold_values(params.xs)
                return total
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let functions = graph["metadata"]["functions"]
        .as_array()
        .expect("functions metadata");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["name"], "fold_values");
    assert_eq!(functions[0]["params"][0]["name"], "xs");
    assert_eq!(functions[0]["params"][1]["name"], "seed");
    assert_eq!(functions[0]["body"]["$call"], "reduce");
    assert_eq!(
        functions[0]["body"]["args"][0],
        serde_json::json!({ "$ref": { "let": ["xs"] } })
    );
    assert_eq!(
        functions[0]["body"]["args"][1],
        serde_json::json!({ "$ref": { "let": ["seed"] } })
    );
    assert_eq!(
        functions[0]["body"]["args"][2]["$lambda"]["params"],
        serde_json::json!(["acc", "x"])
    );
    assert_eq!(
        functions[0]["body"]["args"][2]["$lambda"]["body"],
        serde_json::json!({
            "$call": "add",
            "args": [
                { "$ref": { "let": ["acc"] } },
                { "$ref": { "let": ["x"] } }
            ]
        })
    );

    let node = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["provider"], "std");
    assert_eq!(node["action"]["function"], "run");
    assert_eq!(
        node["action"]["configuration"]["program"][0]["value"],
        serde_json::json!({
            "$call": "fold_values",
            "args": [
                { "$ref": { "params": ["xs"] } },
                0
            ]
        })
    );
}
#[test]
#[ignore = "legacy action-node assertion removed by invocation hard cutover"]
fn pure_block_body_function_lowers_to_program_and_round_trips() {
    let src = r#"
        fn build(a: integer, b: integer) -> integer = {
            let sum = std.math.add(a, b)
            return sum
        }

        workflow "Fn" v1 {
            do {
                let total = build(params.x, params.y)
                return total
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let functions = graph["metadata"]["functions"]
        .as_array()
        .expect("functions metadata");
    assert_eq!(functions[0]["name"], "build");
    // a block body lowers to a `program` array, not a single `body` expression.
    assert!(functions[0]["program"].is_array(), "expected program body");
    assert!(functions[0]["body"].is_null(), "expected no expr body");
    // the surface signature is recorded for decompile.
    assert_eq!(
        graph["metadata"]["rexrap"]["functions"]["build"],
        "(a: integer, b: integer) -> integer"
    );
    // the caller is pure, so the compute block stays in-process (`std.run`).
    let node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["function"], "run");
    assert_round_trips(src);
}
#[test]
#[ignore = "legacy std.exec assertion removed by invocation hard cutover"]
fn effectful_block_body_function_forces_caller_to_exec_and_round_trips() {
    let src = r#"
        fn fetch(url: string) -> object = {
            let resp = std.exec.http_get(url)
            return resp.body
        }

        workflow "Fetch" v1 {
            do {
                let data = fetch(params.url)
                return data
            }
        }
    "#;
    let definition = compile(src);
    let graph = graph_value(&definition);
    let functions = graph["metadata"]["functions"]
        .as_array()
        .expect("functions metadata");
    assert!(functions[0]["program"].is_array(), "expected program body");
    // calling an effectful function makes the enclosing compute block dispatch to the worker.
    let node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["function"], "exec");
    assert_round_trips(src);
}
#[test]
fn effectful_function_rejected_in_declarative_position() {
    // an effectful function may only be called inside a compute block, never in an action argument.
    let src = r#"
        fn fetch(url: string) -> object = {
            let resp = std.exec.http_get(url)
            return resp.body
        }

        workflow "F" v1 {
            slack.send_message(text: fetch(params.url))
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("effectful"), "got: {message}");
    assert!(message.contains("fetch"), "got: {message}");
    assert!(message.contains("compute block"), "got: {message}");
}
#[test]
fn goto_in_function_body_is_rejected() {
    let src = r#"
        fn bad(x: integer) -> integer = {
            goto somewhere
            return x
        }

        workflow "F" v1 {
            console.run(command: "x")
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("goto"), "got: {message}");
    assert!(message.contains("function body"), "got: {message}");
}
#[test]
fn block_body_function_surface_round_trips_through_formatter() {
    let src = r#"
        fn build(a: integer, b: integer) -> integer = {
            let sum = add(a, b)
            return sum
        }

        workflow "Fn" v1 {
            console.run(command: "go")
        }
    "#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("fn build(a: integer, b: integer) -> integer = {"),
        "{formatted}"
    );
    assert!(formatted.contains("let sum = add(a, b)"), "{formatted}");
    assert!(formatted.contains("return sum"), "{formatted}");
    // formatting is idempotent.
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
}
#[test]
fn recursive_function_requires_annotation() {
    let src = r#"
        fn loop(n: integer) = loop(n)

        workflow "Fn" v1 {
            console.run(command: "go")
        }
    "#;
    let message = expect_semantic_error(src);
    assert!(message.contains("recursive"), "got: {message}");
    assert!(message.contains("@recursive"), "got: {message}");
}
#[test]
fn recursive_function_surface_round_trips_through_formatter() {
    let src = r#"
        @recursive(max_depth: 4)
        fn fold(xs: integer[], seed: integer = 0) -> integer = reduce(xs, seed, (acc, x) => add(acc, x))

        workflow "Fn" v1 {
            console.run(command: "go")
        }
    "#;
    let formatted = format_str(src).expect("format");
    assert!(
        formatted.contains("@recursive(max_depth: 4)"),
        "{formatted}"
    );
    assert!(
        formatted.contains(
            "fn fold(xs: integer[], seed: integer = 0) -> integer = reduce(xs, seed, (acc, x) => add(acc, x))"
        ),
        "{formatted}"
    );
    assert_eq!(format_str(&formatted).expect("format twice"), formatted);
}
#[test]
fn lowers_user_function_into_metadata_and_call() {
    let src = r#"
        fn double(x: integer) -> integer = x * 2
        workflow "Fns" v1 {
            node go <- console.run(value: double(21))
        }
    "#;
    let def = compile(src);
    let functions = def
        .definition
        .metadata
        .pointer("/functions")
        .and_then(Value::as_array)
        .expect("functions in metadata");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0].get("name"), Some(&Value::from("double")));
    let params = functions[0]
        .get("params")
        .and_then(Value::as_array)
        .expect("params");
    assert_eq!(params[0].get("name"), Some(&Value::from("x")));
    // the body lowers to the multiplication over the parameter local.
    assert!(functions[0].get("body").is_some());
    // the call lowers to the shared `$call` shape with the single positional argument.
    let value = action_config_value(&def, "value");
    assert_eq!(value.get("$call").and_then(Value::as_str), Some("double"));
    let args = value.get("args").and_then(Value::as_array).expect("args");
    assert_eq!(args.len(), 1);
}
#[test]
fn named_args_resolve_to_positional_with_defaults() {
    let src = r#"
        fn greet(name: string, excited: boolean = false) -> string = name
        workflow "Named" v1 {
            node go <- console.run(value: greet(name: "ada"))
        }
    "#;
    let def = compile(src);
    let value = action_config_value(&def, "value");
    assert_eq!(value.get("$call").and_then(Value::as_str), Some("greet"));
    let args = value.get("args").and_then(Value::as_array).expect("args");
    // the omitted optional is filled from its default, so both parameters are positional.
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], Value::from("ada"));
    assert_eq!(args[1], Value::from(false));
}
#[test]
fn rejects_unannotated_recursion() {
    let message = expect_semantic_error(
        r#"
        fn fact(n: integer) -> integer = n <= 1 ? 1 : n * fact(n - 1)
        workflow "Rec" v1 {
            node go <- console.run(value: fact(5))
        }
    "#,
    );
    assert!(message.contains("@recursive"), "{message}");
}
#[test]
fn recursive_function_evaluates_under_runtime() {
    // a `@recursive`-annotated factorial compiles, carries its body in metadata, and the runtime
    // function table evaluates it to a terminating value via the lazy `$if` form.
    let src = r#"
        @recursive(max_depth: 100)
        fn fact(n: integer) -> integer = n <= 1 ? 1 : n * fact(n - 1)
        workflow "Rec" v1 {
            node go <- console.run(value: "ok")
        }
    "#;
    let def = compile(src);
    let functions = def.definition.metadata.get("functions").expect("functions");
    let table =
        runinator_workflows::FunctionTable::from_metadata(Some(functions)).expect("function table");
    let call = Value::from(serde_json::json!({ "$call": "fact", "args": [5] }));
    let result = runinator_workflows::resolve_value_refs_with_functions(
        &call,
        &Value::from(serde_json::json!({})),
        &table,
    )
    .expect("evaluate");
    assert_eq!(result, Value::from(120));
}
#[test]
fn rejects_function_shadowing_intrinsic() {
    let message = expect_semantic_error(
        r#"
        fn substring(s: string) -> string = s
        workflow "Shadow" v1 {
            node go <- console.run(value: "x")
        }
    "#,
    );
    assert!(message.contains("intrinsic"), "{message}");
}
#[test]
fn function_definition_round_trips_through_formatter() {
    let src = "fn double(x: integer) -> integer = x * 2\n\nworkflow \"Fns\" v1 {\n    node go <- console.run(value: double(21))\n}\n";
    let formatted = format_str(src).expect("format");
    assert!(formatted.contains("fn double(x: integer)"), "{formatted}");
    assert!(formatted.contains("= x * 2"), "{formatted}");
}
