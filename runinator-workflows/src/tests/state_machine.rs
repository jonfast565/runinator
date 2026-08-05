//! whole-workflow behaviour: driving a definition through its states, predicate edge ordering, and
//! normalizing a legacy definition into the current shape.

use super::*;

#[test]
fn test_workflow_state_machine_logic_integration() {
    // 1. define a simple state-machine workflow.
    let definition = runinator_models::json!({
        "start": "start",
        "nodes": [
            {
                "id": "start",
                "kind": "start",
                "transitions": { "next": { "$node": "step1" } }
            },
            {
                "id": "step1",
                "kind": "action",
        "action": {
            "provider": "console",
            "function": "run",
            "timeout_seconds": 60,
            "configuration": {}
        },
                "transitions": { "on_success": { "$node": "step2" }, "on_failure": { "$node": "failed" } }
            },
            {
                "id": "step2",
                "kind": "condition",
                "transitions": {
                    "branches": [{ "when": { "value": { "$ref": { "node": "step1", "output": ["ok"] } }, "equals": true }, "target": { "$node": "success" } }],
                    "next": { "$node": "failed" }
                }
            },
            { "id": "success", "kind": "end" },
            { "id": "failed", "kind": "end" }
        ]
    });

    let wf = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "integration-test".into(),
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(definition.clone()).unwrap(),
        created_at: None,
        updated_at: None,
    };

    // 2. validate the workflow.
    let (start, nodes) = validate_workflow(&wf).expect("Workflow should be valid");
    assert_eq!(start, "start");
    let node_map: HashMap<String, &WorkflowNode> =
        nodes.iter().map(|n| (n.id.clone(), n)).collect();

    // 3. simulate execution - step 1 succeeds.
    let step1_node = node_map.get("step1").unwrap();
    let next = next_transition(
        step1_node,
        WorkflowStatus::Succeeded,
        &runinator_models::json!({}),
    )
    .unwrap();
    assert_eq!(next.unwrap(), "step2");

    // 4. simulate step 2 - condition evaluation.
    let outputs = {
        let mut m = HashMap::new();
        m.insert("step1".to_string(), runinator_models::json!({ "ok": true }));
        m
    };
    let context = outputs_context(&runinator_models::json!({}), &outputs);

    let step2_node = node_map.get("step2").unwrap();
    let next = next_transition(step2_node, WorkflowStatus::Running, &context).unwrap();
    assert_eq!(next.unwrap(), "success");

    // 5. simulate step 2 - condition failure.
    let outputs_fail = {
        let mut m = HashMap::new();
        m.insert(
            "step1".to_string(),
            runinator_models::json!({ "ok": false }),
        );
        m
    };
    let context_fail = outputs_context(&runinator_models::json!({}), &outputs_fail);
    let next_fail = next_transition(step2_node, WorkflowStatus::Running, &context_fail).unwrap();
    assert_eq!(next_fail.unwrap(), "failed");
}

#[test]
fn predicate_edges_route_in_priority_order_on_any_node() {
    // an action node carries two predicate edges; the lower-priority one is evaluated first even
    // though it is declared second.
    let definition = runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "work" } } },
            {
                "id": "work",
                "kind": "action",
                "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} },
                "transitions": {
                    "branches": [
                        { "when": { "value": { "$ref": { "params": ["flag"] } }, "equals": "yes" }, "target": { "$node": "second" }, "priority": 20 },
                        { "when": { "value": { "$ref": { "params": ["flag"] } }, "equals": "yes" }, "target": { "$node": "first" }, "priority": 10 }
                    ],
                    "on_success": { "$node": "fallback" }
                }
            },
            { "id": "first", "kind": "end" },
            { "id": "second", "kind": "end" },
            { "id": "fallback", "kind": "end" }
        ]
    });

    let wf = workflow(definition);
    let (_, nodes) = validate_workflow(&wf).expect("workflow should be valid");
    let node_map: HashMap<String, &WorkflowNode> =
        nodes.iter().map(|n| (n.id.clone(), n)).collect();
    let work = node_map.get("work").unwrap();

    let context = outputs_context(&runinator_models::json!({ "flag": "yes" }), &HashMap::new());

    // both predicates match; priority 10 wins over priority 20 despite later declaration.
    let next = next_transition(work, WorkflowStatus::Succeeded, &context).unwrap();
    assert_eq!(next.unwrap(), "first");

    // with no matching predicate, status routing falls through to on_success.
    let empty = outputs_context(&runinator_models::json!({ "flag": "no" }), &HashMap::new());
    let next = next_transition(work, WorkflowStatus::Succeeded, &empty).unwrap();
    assert_eq!(next.unwrap(), "fallback");
}

#[test]
fn predicate_edges_without_priority_keep_declaration_order() {
    // unset priorities sort last but stable, so declaration order decides between equal matches.
    let definition = runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "work" } } },
            {
                "id": "work",
                "kind": "action",
                "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} },
                "transitions": {
                    "branches": [
                        { "when": null, "target": { "$node": "first" } },
                        { "when": null, "target": { "$node": "second" } }
                    ],
                    "on_success": { "$node": "fallback" }
                }
            },
            { "id": "first", "kind": "end" },
            { "id": "second", "kind": "end" },
            { "id": "fallback", "kind": "end" }
        ]
    });

    let wf = workflow(definition);
    let (_, nodes) = validate_workflow(&wf).expect("workflow should be valid");
    let work = nodes.iter().find(|n| n.id == "work").unwrap();
    let next = next_transition(
        work,
        WorkflowStatus::Succeeded,
        &runinator_models::json!({}),
    )
    .unwrap();
    assert_eq!(next.unwrap(), "first");
}

#[test]
fn normalizes_legacy_workflow_with_start_and_end_nodes() {
    let wf = workflow(runinator_models::json!({
        "start": "build",
        "nodes": [
            { "id": "build", "kind": "action", "action": { "provider": "console", "function": "run", "timeout_seconds": 60, "configuration": {} }, "transitions": {} }
        ],
        "ui": {
            "layout": {
                "build": { "x": 10, "y": 20 }
            }
        }
    }));

    let normalized = normalize_workflow(&wf);
    let definition = normalized.definition.as_value();
    let definition = definition.as_object().unwrap();
    assert_eq!(definition["start"], "start");
    assert_eq!(definition["ui"]["layout"]["nodes"]["build"]["x"], 10);
    let (_, nodes) = validate_workflow(&normalized).expect("normalized workflow is valid");
    assert!(
        nodes
            .iter()
            .any(|node| node.kind == WorkflowNodeKind::Start)
    );
    assert!(nodes.iter().any(|node| node.kind == WorkflowNodeKind::End));
    assert!(nodes.iter().any(|node| node.kind == WorkflowNodeKind::Fail));
    let build = nodes.iter().find(|node| node.id == "build").unwrap();
    assert_eq!(
        build
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str()),
        Some("end")
    );
}
