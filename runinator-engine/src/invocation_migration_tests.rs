//! the stored-definition converter.
//!
//! the assertions worth having are the ones about what it *refuses* to touch: a converter that
//! silently rewrote a `std.code` node, or that produced a definition the decompiler could no longer
//! render, would only be discovered by an operator whose fleet was already half-migrated.

use super::*;

fn definition(graph: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "name": "migrated",
        "version": "1.0.0",
        "enabled": true,
        "definition": graph,
    }))
    .expect("workflow definition")
}

fn compute_node(function: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "step",
        "kind": "action",
        "action": {
            "provider": "std",
            "function": function,
            "timeout_seconds": 45,
            "configuration": { "program": [{ "$return": 1 }] }
        },
        "transitions": { "on_success": { "$node": "done" } }
    })
}

#[test]
fn a_std_run_node_becomes_an_invocation() {
    let workflow = definition(serde_json::json!({
        "start": "step",
        "nodes": [compute_node("run"), { "id": "done", "kind": "end" }]
    }));
    let converted = convert_definition(&workflow)
        .expect("converts")
        .expect("changed");
    let graph = serde_json::to_value(&converted.definition).unwrap();
    let node = &graph["nodes"][0];

    assert_eq!(node["kind"], "invocation");
    assert!(
        node["action"].is_null(),
        "the action is replaced, not kept alongside"
    );
    assert_eq!(node["parameters"]["module"]["version"], 1);
    assert_eq!(node["parameters"]["timeout_seconds"], 45);
    // the retained tree is what keeps a converted definition decompilable.
    assert_eq!(
        node["parameters"]["source"],
        serde_json::json!([{ "$return": 1 }])
    );
    // the transitions are the node's, not the converter's, and must survive untouched.
    assert_eq!(node["transitions"]["on_success"]["$node"], "done");
}

#[test]
fn a_std_exec_node_becomes_an_invocation_too() {
    let workflow = definition(serde_json::json!({
        "start": "step",
        "nodes": [compute_node("exec"), { "id": "done", "kind": "end" }]
    }));
    let converted = convert_definition(&workflow)
        .expect("converts")
        .expect("changed");
    let graph = serde_json::to_value(&converted.definition).unwrap();
    assert_eq!(graph["nodes"][0]["kind"], "invocation");
}

#[test]
fn a_std_code_node_is_left_alone() {
    // foreign source has no program to assemble, and the node already dispatches exactly once and
    // settles — converting it would buy nothing and lose the container configuration.
    let workflow = definition(serde_json::json!({
        "start": "step",
        "nodes": [
            {
                "id": "step",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "code",
                    "timeout_seconds": 60,
                    "configuration": { "language": "python", "source": "pass" }
                },
                "transitions": {}
            },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(convert_definition(&workflow).expect("converts").is_none());
}

#[test]
fn an_ordinary_provider_action_is_left_alone() {
    let workflow = definition(serde_json::json!({
        "start": "step",
        "nodes": [
            {
                "id": "step",
                "kind": "action",
                "action": {
                    "provider": "http",
                    "function": "get",
                    "timeout_seconds": 30,
                    "configuration": {}
                },
                "transitions": {}
            },
            { "id": "done", "kind": "end" }
        ]
    }));
    assert!(convert_definition(&workflow).expect("converts").is_none());
}

#[test]
fn conversion_is_idempotent() {
    // running it twice must be a no-op the second time: an operator who reruns it after a partial
    // failure elsewhere should not get a second rewrite.
    let workflow = definition(serde_json::json!({
        "start": "step",
        "nodes": [compute_node("run"), { "id": "done", "kind": "end" }]
    }));
    let once = convert_definition(&workflow)
        .expect("converts")
        .expect("changed");
    assert!(convert_definition(&once).expect("converts").is_none());
}

#[test]
fn stored_functions_move_into_the_module() {
    let workflow = definition(serde_json::json!({
        "start": "step",
        "nodes": [
            {
                "id": "step",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "run",
                    "timeout_seconds": 60,
                    "configuration": {
                        "program": [{ "$return": { "$call": "double", "args": [4] } }]
                    }
                },
                "transitions": {}
            },
            { "id": "done", "kind": "end" }
        ],
        "metadata": {
            "functions": [{
                "name": "double",
                "params": [{ "name": "n" }],
                "body": { "$call": "mul", "args": [{ "$ref": { "let": ["n"] } }, 2] }
            }]
        }
    }));
    let converted = convert_definition(&workflow)
        .expect("converts")
        .expect("changed");
    let graph = serde_json::to_value(&converted.definition).unwrap();

    let functions = graph["nodes"][0]["parameters"]["module"]["functions"]
        .as_array()
        .expect("the module carries the function");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["name"], "double");
    // the executable bodies move; nothing is left behind for the engine to call.
    assert!(graph["metadata"]["functions"].is_null());
}

#[test]
fn a_report_with_a_blocked_definition_is_not_clear() {
    let mut report = MigrationReport {
        convertible: vec!["a".into()],
        ..Default::default()
    };
    assert!(report.is_clear());
    report.blocked.push(("b".into(), "bad program".into()));
    assert!(
        !report.is_clear(),
        "one blocked definition must stop the whole migration"
    );
}
