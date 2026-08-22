use super::*;

#[test]
fn language_style_task_binding_lowers_as_an_action_node() {
    let definition = compile(
        r#"language rexrap-1

workflow "Smoke" v1 {

    do {
        let result: task[SmokeResult] = monitoring.smoke(target: "api")
    }
}"#,
    );

    let action = definition
        .definition
        .nodes
        .iter()
        .find(|node| node.id == "result")
        .expect("bound action node");
    assert_eq!(
        action.kind,
        runinator_models::workflows::WorkflowNodeKind::Action
    );
    let graph = serde_json::to_value(&definition.definition).expect("serialize graph");
    assert_eq!(
        graph["metadata"]["rexrap"]["types"]["result"],
        "task[SmokeResult]"
    );
    let result = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["id"] == "result")
        .expect("result node");
    assert_eq!(result["parameters"]["rexrap_task"], true);
}

#[test]
fn provider_task_is_joined_by_its_durable_task_handle() {
    let definition = compile(
        r#"language rexrap-1

workflow "Smoke" v1 {

    do {
        let result: task[SmokeResult] = monitoring.smoke(target: "api")
        await result
    }
}"#,
    );
    let graph = serde_json::to_value(&definition.definition).expect("serialize graph");
    let await_node = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["kind"] == "await_run")
        .expect("await node");
    assert_eq!(
        await_node["parameters"]["task_run_id"]["$ref"]["node"],
        "result"
    );
    assert_eq!(
        await_node["parameters"]["task_run_id"]["$ref"]["output"],
        serde_json::json!(["task_run_id"])
    );
}

#[test]
fn detached_subflow_task_is_joined_by_its_binding_name() {
    let source = r#"language rexrap-1

workflow "Parent" v1 {

    do {
        let smoke: task = subflow("Child", detached: true)
        await smoke
    }
}"#;
    assert_round_trips(source);
    let definition = compile(source);

    let graph = serde_json::to_value(&definition.definition).expect("serialize graph");
    let await_node = graph["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .find(|node| node["kind"] == "await_run")
        .expect("await node");
    assert_eq!(await_node["parameters"]["run_id"]["$ref"]["node"], "smoke");
    assert_eq!(
        await_node["parameters"]["run_id"]["$ref"]["output"],
        serde_json::json!(["subflow_run_id"])
    );
}

#[test]
fn compute_blocks_interleave_with_language_style_bindings() {
    compile(
        r#"language rexrap-1

workflow "Compute" v1 {

    do {
        let prepared: integer = compute {
            let base = 40
            return base + 2
        }
        let sent = console.run(command: prepared)
    }
}"#,
    );
}

#[test]
fn language_routes_accept_end_and_fail_terminals() {
    compile(
        r#"language rexrap-1

workflow "Routes" v1 {

    do {
        let action = console.run(command: "go")
        routes {
            on success {
                continue end
            }
            on failure {
                continue fail
            }
        }
    }
}"#,
    );
}
