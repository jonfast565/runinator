//! `do` blocks: lowering pure bodies to `std.run` and effectful ones to `std.exec`, and the
//! type checking over both.

use super::*;

#[test]
#[ignore = "legacy std.run assertion removed by invocation hard cutover"]
fn compute_pure_lowers_to_std_run() {
    let src = r#"
        workflow "Compute" v1 {
            do {
                let total = prev.cart.subtotal + prev.cart.tax
                if total <= 0 { goto fail }
                return { total: total }
            }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["provider"], "std");
    assert_eq!(node["action"]["function"], "run");
    assert!(node["action"]["configuration"]["program"].is_array());
    assert_round_trips(src);
}
#[test]
fn foreign_compute_lowers_to_std_code_and_round_trips() {
    let src = r#"
        workflow "Foreign Compute" v1 {
            node result: { total: integer } <- do "python" ```
def main(context):
    return {"total": context["input"]["a"] + 1}
```.timeout(45s)
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["provider"], "std");
    assert_eq!(node["action"]["function"], "code");
    assert_eq!(node["action"]["timeout_seconds"], 45);
    assert_eq!(node["action"]["configuration"]["language"], "python");
    assert_eq!(
        value["metadata"]["rexrap"]["type_hints"]["result"]["fields"]["total"]["ty"]["type"],
        "integer"
    );
    assert!(node["action"]["configuration"]["image"].is_null());
    assert!(
        node["action"]["configuration"]["source"]
            .as_str()
            .unwrap()
            .contains("def main(context)")
    );

    let rexrap = decompile(&definition).expect("decompile");
    assert!(rexrap.contains("do \"python\""), "{rexrap}");
    assert!(!rexrap.contains("using"), "{rexrap}");
    assert!(rexrap.contains("def main(context)"), "{rexrap}");
    let second = compile_str(&rexrap, &default_test_options()).expect("recompile");
    assert_eq!(graph_value(&definition), graph_value(&second));
}
#[test]
fn foreign_compute_keeps_restored_language_alias_as_string() {
    let src = r#"
        workflow "Foreign Compute Alias" v1 {
            node result <- do "js" ```
console.log(JSON.stringify({ total: 42 }))
```
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["provider"], "std");
    assert_eq!(node["action"]["function"], "code");
    assert_eq!(node["action"]["configuration"]["language"], "js");
    assert!(node["action"]["configuration"]["image"].is_null());

    let rexrap = decompile(&definition).expect("decompile");
    assert!(rexrap.contains("do \"js\""), "{rexrap}");
    assert!(!rexrap.contains("using"), "{rexrap}");
    let second = compile_str(&rexrap, &default_test_options()).expect("recompile");
    assert_eq!(graph_value(&definition), graph_value(&second));
}
#[test]
#[ignore = "legacy action-node assertion removed by invocation hard cutover"]
fn compute_lambda_map_lowers_and_round_trips() {
    let src = r#"
        workflow "Map" v1 {
            do {
                let names = std.collections.map(params.users, u => u.name)
                return { names: names }
            }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    // a higher-order call with a pure body stays pure (`std.run`).
    assert_eq!(node["action"]["function"], "run");
    let program = node["action"]["configuration"]["program"].to_string();
    assert!(program.contains("$lambda"), "program: {program}");
    assert!(program.contains("\"map\""), "program: {program}");
    assert_round_trips(src);
}
#[test]
fn compute_lambda_filter_reduce_round_trip() {
    // filter/reduce drive predicates and folds through expression-level intrinsics (gt/add).
    let src = r#"
        workflow "Pipe" v1 {
            do {
                let big = std.collections.filter(params.xs, x => std.logic.gt(x, 1))
                let total = std.collections.reduce(big, 0, (acc, x) => std.math.add(acc, x))
                return { total: total }
            }
        }
    "#;
    assert_round_trips(src);
}
#[test]
#[ignore = "legacy std.exec assertion removed by invocation hard cutover"]
fn compute_effectful_lowers_to_std_exec() {
    let src = r#"
        workflow "Fetch" v1 {
            do {
                let resp = std.exec.http_get(params.url)
                return { status: resp.status }
            }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .expect("compute action node");
    assert_eq!(node["action"]["function"], "exec");
    assert_round_trips(src);
}
#[test]
fn compute_rejects_goto_in_effectful_block() {
    let src = r#"
        workflow "Bad" v1 {
            do {
                let resp = std.exec.http_get(params.url)
                if resp.status > 0 { goto fail }
                return resp
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("goto"), "unexpected message: {message}");
}
#[test]
fn compute_rejects_unknown_intrinsic() {
    let src = r#"
        workflow "Typo" v1 {
            do { return addd(1, 2) }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("unknown function"), "got: {message}");
}
#[test]
fn compute_rejects_bad_arity() {
    let src = r#"
        workflow "Arity" v1 {
            do { return std.math.add(1) }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("argument"), "got: {message}");
}
#[test]
fn compute_rejects_let_type_mismatch() {
    let src = r#"
        workflow "Mismatch" v1 {
            do { let x: integer = "hello" return x }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("integer"), "got: {message}");
}
#[test]
fn compute_rejects_bad_argument_type() {
    let src = r#"
        workflow "BadArg" v1 {
            do { return std.math.add("a", 1) }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("argument"), "got: {message}");
}
#[test]
fn compute_lambda_uses_collection_item_type_for_field_access() {
    let src = r#"
        workflow "LambdaTypes" v1 {
            params { users: { id: string }[] }
            do {
                return std.collections.map(params.users, u => u.missing)
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(
        message.contains("unknown field 'missing'"),
        "got: {message}"
    );
}
#[test]
fn compute_lambda_result_drives_higher_order_return_type() {
    let src = r#"
        workflow "LambdaReturn" v1 {
            params { users: { id: string }[] }
            do {
                let ids: integer[] = std.collections.map(params.users, u => u.id)
                return ids
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'ids'"), "got: {message}");
    assert!(
        message.contains("expected integer, got string"),
        "got: {message}"
    );
}
#[test]
fn compute_first_recovers_element_type() {
    // `first` of a string[] resolves to string, so assigning it to an integer local is an error.
    let src = r#"
        workflow "FirstTyped" v1 {
            params { items: string[] }
            do {
                let x: integer = std.collections.first(params.items)
                return x
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'x'"), "got: {message}");
    assert!(
        message.contains("expected integer, got string"),
        "got: {message}"
    );
}
#[test]
fn compute_sort_preserves_element_type() {
    // `sort` preserves the element type, so sorting a string[] and binding it to integer[] errors.
    let src = r#"
        workflow "SortTyped" v1 {
            params { items: string[] }
            do {
                let sorted: integer[] = std.collections.sort(params.items)
                return sorted
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'sorted'"), "got: {message}");
    assert!(message.contains("string"), "got: {message}");
}
#[test]
fn compute_at_struct_recovers_field_type_from_literal_key() {
    // at(struct, "id") resolves to the field type when the key is a literal.
    let src = r#"
        workflow "AtStruct" v1 {
            params { obj: { id: integer } }
            do {
                let x: string = std.collections.at(params.obj, "id")
                return x
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'x'"), "got: {message}");
    assert!(
        message.contains("expected string, got integer"),
        "got: {message}"
    );
}
#[test]
fn compute_pick_narrows_struct_to_named_keys() {
    // pick keeps only the named keys, so the dropped field is unknown afterwards.
    let src = r#"
        workflow "Pick" v1 {
            params { obj: { id: integer, secret: string } }
            do {
                let picked = std.objects.pick(params.obj, ["id"])
                return picked.secret
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("unknown field 'secret'"), "got: {message}");
}
#[test]
fn run_context_field_is_typed_string() {
    // run.run_id is a string; assigning it to an integer local is an error.
    let src = r#"
        workflow "RunCtx" v1 {
            do {
                let x: integer = run.run_id
                return x
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'x'"), "got: {message}");
    assert!(message.contains("string"), "got: {message}");
}
#[test]
fn run_context_rejects_unknown_field() {
    // the run context is closed to its known keys; `run.name` does not exist.
    let src = r#"
        workflow "RunCtx" v1 {
            do {
                return run.name
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("unknown field 'name'"), "got: {message}");
}
#[test]
fn typed_parse_json_adopts_annotated_shape() {
    // a `let` annotation gives parse_json's `any` result a concrete shape, so a wrong downstream
    // use is caught (x.id is integer, not string).
    let src = r#"
        workflow "P" v1 {
            params { s: string }
            do {
                let x: { id: integer } = std.encoding.parse_json(params.s)
                let y: string = x.id
                return y
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'y'"), "got: {message}");
    assert!(
        message.contains("expected string, got integer"),
        "got: {message}"
    );
}
#[test]
fn empty_array_adopts_annotated_element_type() {
    // `[]` under a declaration resolves to the declared element type, so `first(x)` is that element.
    let src = r#"
        workflow "P" v1 {
            do {
                let x: string[] = []
                let y: integer = std.collections.first(x)
                return y
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'y'"), "got: {message}");
    assert!(message.contains("string"), "got: {message}");
}
#[test]
fn first_class_lambda_call_yields_result_type() {
    // a lambda bound to a local infers a function type; calling it yields the body's result type,
    // so assigning a string-returning lambda's result to an integer is an error.
    let src = r#"
        workflow "F" v1 {
            do {
                let f = x => "hello"
                let y: integer = f(2)
                return y
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("compute local 'y'"), "got: {message}");
    assert!(
        message.contains("expected integer, got string"),
        "got: {message}"
    );
}
#[test]
fn first_class_lambda_round_trips() {
    // a let-bound lambda, applied by name and passed to a higher-order intrinsic, survives
    // compile -> decompile -> compile.
    assert_round_trips(
        r#"
        workflow "F" v1 {
            params { xs: integer[] }
            do {
                let double = x => std.math.mul(x, 2)
                return std.collections.map(params.xs, double)
            }
        }
    "#,
    );
}
#[test]
fn first_class_lambda_call_checks_arity() {
    let src = r#"
        workflow "F" v1 {
            do {
                let f = x => x
                return f(1, 2)
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(
        message.contains("'f' expects 1 argument(s), got 2"),
        "got: {message}"
    );
}
#[test]
fn compute_predicate_lambda_must_return_boolean() {
    let src = r#"
        workflow "LambdaPredicate" v1 {
            params { users: { id: string }[] }
            do {
                return std.collections.filter(params.users, u => u.id)
            }
        }
    "#;
    let (_, message) = expect_semantic(src);
    assert!(message.contains("boolean"), "got: {message}");
    assert!(message.contains("string"), "got: {message}");
}
#[test]
fn compute_accepts_well_typed_program() {
    // a correctly typed program with annotations and a call result flows cleanly.
    let src = r#"
        workflow "Typed" v1 {
            params { a: integer, b: integer }
            do {
                let sum: number = std.math.add(params.a, params.b)
                return sum
            }
        }
    "#;
    assert_round_trips(src);
}
#[test]
#[ignore = "legacy std.exec assertion removed by invocation hard cutover"]
fn compute_secret_reference_forces_exec() {
    let src = r#"
        workflow "Sec" v1 {
            do { return secret.api.key }
        }
    "#;
    let definition = compile(src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap();
    // a secret reference can only resolve at the worker, so the block must be exec.
    assert_eq!(node["action"]["function"], "exec");
}
#[test]
#[ignore = "legacy action-node assertion removed by invocation hard cutover"]
fn compute_condition_allows_arithmetic_and_calls() {
    // arithmetic in a pure condition, and a call (which makes the block exec).
    let pure_src = r#"
        workflow "PureCond" v1 {
            do {
                let total = params.a + params.b
                if total * 2 > 100 { goto fail }
                return total
            }
        }
    "#;
    assert_round_trips(pure_src);

    let call_src = r#"
        workflow "CallCond" v1 {
            do {
                if std.collections.len(params.items) > 0 {
                    return std.exec.http_get(params.url)
                }
                return null
            }
        }
    "#;
    let definition = compile(call_src);
    let value = serde_json::to_value(&definition.definition).unwrap();
    let node = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "action")
        .unwrap();
    assert_eq!(node["action"]["function"], "exec");
    assert_round_trips(call_src);
}
#[test]
fn compute_arithmetic_round_trips() {
    // arithmetic and library calls work both as let/return values and inside object/array literals.
    let src = r#"
        workflow "Math" v1 {
            do {
                let x = (params.a + params.b) * 2 - params.c
                return { x: x, y: std.math.add(x, 1), zs: [x, x * 2] }
            }
        }
    "#;
    assert_round_trips(src);
}

// --- the invocation backend -------------------------------------------------------------------
//
// Every `do { }` block compiles to an invocation node carrying assembled bytecode.

fn compile_as_invocation(src: &str) -> runinator_models::workflows::WorkflowDefinition {
    let options = CompileOptions {
        ..default_test_options()
    };
    compile_str(src, &options).expect("compile with invocations")
}

fn invocation_node(
    definition: &runinator_models::workflows::WorkflowDefinition,
) -> serde_json::Value {
    let value = serde_json::to_value(&definition.definition).unwrap();
    value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["kind"] == "invocation")
        .cloned()
        .expect("an invocation node")
}

#[test]
fn a_do_block_compiles_to_an_invocation_node_carrying_a_module() {
    let src = r#"
        workflow "Compute" v1 {
            do {
                let total = prev.cart.subtotal + prev.cart.tax
                return { total: total }
            }
        }
    "#;
    let node = invocation_node(&compile_as_invocation(src));
    let module = &node["parameters"]["module"];
    assert_eq!(module["version"], 1);
    assert!(
        module["entry"]["instructions"]
            .as_array()
            .is_some_and(|instructions| !instructions.is_empty()),
        "the module carries assembled instructions"
    );
}

#[test]
fn an_invocation_node_retains_its_source_for_decompiling() {
    // the module is bytecode; recovering `let`/`if`/`return` from it would be control-flow
    // reconstruction. the retained tree is what the decompiler renders, which is why the round trip
    // below can succeed at all.
    let src = r#"
        workflow "Compute" v1 {
            do {
                let total = prev.cart.subtotal + prev.cart.tax
                if total <= 0 { goto fail }
                return { total: total }
            }
        }
    "#;
    let node = invocation_node(&compile_as_invocation(src));
    assert!(
        node["parameters"]["source"].is_array(),
        "an invocation retains the statement tree it was assembled from"
    );
}

#[test]
fn an_invocation_round_trips_back_to_the_same_source() {
    let src = r#"
        workflow "Compute" v1 {
            do {
                let total = prev.cart.subtotal + prev.cart.tax
                if total <= 0 { goto fail }
                return { total: total }
            }
        }
    "#;
    let options = CompileOptions {
        ..default_test_options()
    };
    let definition = compile_str(src, &options).expect("compile");
    let rendered = crate::decompile(&definition).expect("decompile");
    // decompiling and recompiling must reach the same definition: that is what makes the editor's
    // save path safe, and it is the contract the retained source exists to keep.
    let again = compile_str(&rendered, &options).expect("recompile the decompiled source");
    assert_eq!(
        serde_json::to_value(&definition.definition).unwrap(),
        serde_json::to_value(&again.definition).unwrap(),
        "an invocation definition did not survive a decompile/recompile round trip"
    );
}

#[test]
fn user_functions_are_compiled_into_the_module() {
    let src = r#"
        fn double(n: integer) -> integer = n * 2

        workflow "Compute" v1 {
            do {
                return double(prev.value)
            }
        }
    "#;
    let node = invocation_node(&compile_as_invocation(src));
    let functions = node["parameters"]["module"]["functions"]
        .as_array()
        .expect("the module carries its functions");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["name"], "double");
    assert_eq!(functions[0]["params"][0], "n");
}

#[test]
#[ignore = "legacy default assertion removed by invocation hard cutover"]
fn the_default_still_emits_a_std_run_node() {
    // the flip is opt-in. a default compile must keep producing what every stored definition and
    // every running replica already understands.
    let src = r#"
        workflow "Compute" v1 {
            do { return 1 }
        }
    "#;
    let value = serde_json::to_value(&compile(src).definition).unwrap();
    let kinds: Vec<_> = value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["kind"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(kinds.contains(&"action".to_string()));
    assert!(!kinds.contains(&"invocation".to_string()));
}
