use std::collections::HashMap;

use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowDefinition, WorkflowStatus};

use crate::simulate::{NodeEvalRequest, NodeOutcome, SimulationEnv, simulate_workflow};

fn workflow(nodes: serde_json::Value, start: &str) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "name": "sim-test",
        "definition": { "start": start, "nodes": nodes },
    }))
    .expect("workflow definition")
}

// an env that returns a fixed outcome for named nodes, else success with null.
struct FixedEnv {
    config: Value,
    outcomes: HashMap<String, NodeOutcome>,
}

impl FixedEnv {
    fn new() -> Self {
        Self {
            config: Value::Null,
            outcomes: HashMap::new(),
        }
    }

    fn action(mut self, node: &str, output: Value) -> Self {
        self.outcomes
            .insert(node.to_string(), NodeOutcome::succeeded(output));
        self
    }
}

impl SimulationEnv for FixedEnv {
    fn config_tree(&mut self) -> Value {
        self.config.clone()
    }

    fn evaluate_action(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome {
        self.outcomes
            .get(&request.node.id)
            .cloned()
            .unwrap_or_else(|| NodeOutcome::succeeded(Value::Null))
    }
}

#[test]
fn toggle_routes_on_truthy_value() {
    let def = workflow(
        serde_json::json!([
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "flag" } } },
            {
                "id": "flag",
                "kind": "toggle",
                "parameters": {
                    "value": { "$ref": { "input": ["enabled"] } },
                    "on": { "$node": "yes" },
                    "off": { "$node": "no" }
                }
            },
            { "id": "yes", "kind": "end" },
            { "id": "no", "kind": "end" }
        ]),
        "start",
    );

    let mut env = FixedEnv::new();
    let run = simulate_workflow(&def, runinator_models::json!({ "enabled": true }), &mut env)
        .expect("simulate");
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    assert_eq!(run.branch_target("flag"), Some("yes"));
    assert!(run.reached("yes"));
    assert!(!run.reached("no"));

    let mut env = FixedEnv::new();
    let run = simulate_workflow(
        &def,
        runinator_models::json!({ "enabled": false }),
        &mut env,
    )
    .expect("simulate");
    assert_eq!(run.branch_target("flag"), Some("no"));
    assert!(run.reached("no"));
}

#[test]
fn action_output_feeds_a_downstream_condition() {
    let def = workflow(
        serde_json::json!([
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "charge" } } },
            {
                "id": "charge",
                "kind": "action",
                "action": { "provider": "console", "function": "log", "configuration": {} },
                "transitions": { "next": { "$node": "gate" } }
            },
            {
                "id": "gate",
                "kind": "condition",
                "condition": { "value": { "$ref": { "node": "charge", "output": ["ok"] } } },
                "transitions": {
                    "on_success": { "$node": "notify" },
                    "on_failure": { "$node": "reject" }
                }
            },
            { "id": "notify", "kind": "end" },
            { "id": "reject", "kind": "fail" }
        ]),
        "start",
    );

    let mut env = FixedEnv::new().action("charge", runinator_models::json!({ "ok": true }));
    let run = simulate_workflow(&def, Value::Null, &mut env).expect("simulate");
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    assert!(run.reached("notify"));
    assert!(!run.reached("reject"));

    let mut env = FixedEnv::new().action("charge", runinator_models::json!({ "ok": false }));
    let run = simulate_workflow(&def, Value::Null, &mut env).expect("simulate");
    assert_eq!(run.status, WorkflowStatus::Failed);
    assert!(run.reached("reject"));
}

#[test]
fn output_node_sets_run_output() {
    let def = workflow(
        serde_json::json!([
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "out" } } },
            {
                "id": "out",
                "kind": "output",
                "parameters": { "data": { "$ref": { "input": ["amount"] } } },
                "transitions": { "next": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]),
        "start",
    );

    let mut env = FixedEnv::new();
    let run = simulate_workflow(&def, runinator_models::json!({ "amount": 42 }), &mut env)
        .expect("simulate");
    assert_eq!(run.output, runinator_models::json!(42));
    assert_eq!(run.status, WorkflowStatus::Succeeded);
}

#[test]
fn unsupported_kind_stops_with_error() {
    let def = workflow(
        serde_json::json!([
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fan" } } },
            {
                "id": "fan",
                "kind": "parallel",
                "parameters": { "branches": [ { "$node": "work" } ] }
            },
            {
                "id": "work",
                "kind": "action",
                "action": { "provider": "console", "function": "log", "configuration": {} },
                "transitions": { "next": { "$node": "gather" } }
            },
            {
                "id": "gather",
                "kind": "join",
                "parameters": { "wait_for": [ { "$node": "work" } ] },
                "transitions": { "next": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]),
        "start",
    );

    let mut env = FixedEnv::new();
    let run = simulate_workflow(&def, Value::Null, &mut env).expect("simulate");
    assert!(run.error.is_some());
    assert!(run.error.unwrap().contains("parallel"));
}
