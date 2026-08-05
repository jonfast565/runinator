//! reference resolution and condition evaluation: what `$ref`/`$template` expand to, and what the
//! condition evaluator makes of the result.

use super::*;

#[test]
fn resolves_value_refs() {
    let context = runinator_models::json!({
        "steps": { "find": { "output": { "items": [{ "key": "A-1" }] } } }
    });
    let value =
        runinator_models::json!({ "$ref": { "node": "find", "output": ["items", 0, "key"] } });
    assert_eq!(
        resolve_value_refs(&value, &context).unwrap(),
        runinator_models::value::Value::String("A-1".into())
    );
}

#[test]
fn resolves_node_artifact_refs() {
    // artifacts live at the step root (sibling of `output`); a `node.artifacts` ref falls back to
    // the step root when the output path misses, so artifacts are referenceable downstream.
    let context = runinator_models::json!({
        "steps": {
            "dump": {
                "output": { "rows": 3 },
                "artifacts": [{ "id": "abc", "uri": "/tmp/dump.csv" }]
            }
        }
    });
    let value =
        runinator_models::json!({ "$ref": { "node": "dump", "output": ["artifacts", 0, "uri"] } });
    assert_eq!(
        resolve_value_refs(&value, &context).unwrap(),
        runinator_models::value::Value::String("/tmp/dump.csv".into())
    );
    // a real output key still wins over the step-root fallback.
    let rows = runinator_models::json!({ "$ref": { "node": "dump", "output": ["rows"] } });
    assert_eq!(
        resolve_value_refs(&rows, &context).unwrap(),
        runinator_models::value::Value::from(3)
    );
}

#[test]
fn resolves_config_refs() {
    // config is injected into the context by the web service as `{ scope: { name: value } }`.
    let context = runinator_models::json!({
        "config": { "api": { "settings": { "url": "https://example.test" } } }
    });
    let value = runinator_models::json!({ "$ref": { "config": ["api", "settings", "url"] } });
    assert_eq!(
        resolve_value_refs(&value, &context).unwrap(),
        runinator_models::value::Value::String("https://example.test".into())
    );
}

#[test]
fn accepts_structurally_valid_refs_without_schema_path_validation() {
    let wf = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "schema-boundary".into(),
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: RuninatorType::from_json_schema(&runinator_models::json!({
            "type": "object",
            "properties": {
                "known": { "type": "string" }
            }
        })),
        definition: WorkflowGraph::from_value(runinator_models::json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "produce" } } },
                {
                    "id": "produce",
                    "kind": "output",
                    "parameters": {
                        "data": { "ok": true }
                    },
                    "transitions": { "next": { "$node": "consume" } }
                },
                {
                    "id": "consume",
                    "kind": "output",
                    "parameters": {
                        "data": {
                            "input": { "$ref": { "params": ["not_in_input_type"] } },
                            "output": { "$ref": { "node": "produce", "output": ["not_in_result_metadata"] } }
                        }
                    },
                    "transitions": { "next": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };

    validate_workflow(&wf).expect("schema path validation is out of scope");
}

#[test]
fn resolves_template_refs() {
    let context = runinator_models::json!({
        "prev": { "ticket_id": "RUN-123", "count": 3 }
    });

    assert_eq!(
        resolve_value_refs(
            &runinator_models::json!({ "$concat": ["Ticket ", { "$ref": { "prev": ["ticket_id"] } }] }),
            &context
        )
        .unwrap(),
        runinator_models::value::Value::String("Ticket RUN-123".into())
    );
    assert_eq!(
        resolve_value_refs(
            &runinator_models::json!({ "$ref": { "prev": ["count"] } }),
            &context
        )
        .unwrap(),
        runinator_models::value::Value::from(3)
    );
}

#[test]
fn expands_local_defs_with_overlay() {
    let wf = workflow(runinator_models::json!({
        "$defs": {
            "approval": { "kind": "approval", "parameters": { "type": "merge" } }
        },
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "approve" } } },
            { "id": "approve", "$ref": "#/$defs/approval", "with": { "parameters": { "prompt": "ok?" } }, "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }));

    let (_, nodes) = parse_nodes(&wf).unwrap();
    assert_eq!(nodes[1].kind, WorkflowNodeKind::Approval);
    assert_eq!(nodes[1].parameters["type"], "merge");
    assert_eq!(nodes[1].parameters["prompt"], "ok?");
}

#[test]
fn evaluates_conditions() {
    let context = runinator_models::json!({
        "input": { "env": "prod" },
        "steps": { "check": { "output": { "status": "ok", "count": 10 } } }
    });

    // simple equality.
    let cond1 = runinator_models::json!({ "value": { "$ref": { "node": "check", "output": ["status"] } }, "equals": "ok" });
    assert!(evaluate_condition(&cond1, &context).unwrap());

    // logical all (and).
    let cond3 = runinator_models::json!({
        "all": [
            { "value": { "$ref": { "params": ["env"] } }, "equals": "prod" },
            { "value": { "$ref": { "node": "check", "output": ["status"] } }, "equals": "ok" }
        ]
    });
    assert!(evaluate_condition(&cond3, &context).unwrap());

    // logical any (or).
    let cond4 = runinator_models::json!({
        "any": [
            { "value": { "$ref": { "params": ["env"] } }, "equals": "dev" },
            { "value": { "$ref": { "node": "check", "output": ["count"] } }, "equals": 10 }
        ]
    });
    assert!(evaluate_condition(&cond4, &context).unwrap());
}

#[test]
fn evaluates_richer_conditions() {
    let context = runinator_models::json!({
        "input": {
            "ticket": "ITP-123",
            "labels": ["auto-implement", "backend"],
            "fields": { "priority": "high" },
            "score": 7
        }
    });

    for condition in [
        runinator_models::json!({ "value": { "$ref": { "params": ["ticket"] } }, "starts_with": "ITP-" }),
        runinator_models::json!({ "value": { "$ref": { "params": ["ticket"] } }, "ends_with": "123" }),
        runinator_models::json!({ "value": { "$ref": { "params": ["ticket"] } }, "contains": "TP-1" }),
        runinator_models::json!({ "value": { "$ref": { "params": ["labels"] } }, "contains": "auto-implement" }),
        runinator_models::json!({ "value": { "$ref": { "params": ["fields"] } }, "contains": "priority" }),
        runinator_models::json!({ "value": { "$ref": { "params": ["ticket"] } }, "in": ["OPS-1", "ITP-123"] }),
        runinator_models::json!({ "value": { "$ref": { "params": ["score"] } }, "greater_than": 5 }),
        runinator_models::json!({ "value": { "$ref": { "params": ["score"] } }, "less_than_or_equal": 7 }),
    ] {
        assert!(
            evaluate_condition(&condition, &context).unwrap(),
            "{condition}"
        );
    }
}

#[test]
fn evaluates_truthy_value_conditions() {
    let context = runinator_models::json!({});

    let truthy = runinator_models::json!({ "value": { "$add": [1, 1] } });
    let falsy = runinator_models::json!({ "value": { "$sub": [1, 1] } });
    let boolean = runinator_models::json!({ "value": true });

    assert!(evaluate_condition(&truthy, &context).unwrap());
    assert!(!evaluate_condition(&falsy, &context).unwrap());
    assert!(evaluate_condition(&boolean, &context).unwrap());
}
