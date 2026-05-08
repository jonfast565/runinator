use crate::context::*;
use runinator_models::workflows::{WorkflowRun, WorkflowNodeRun, WorkflowStatus};
use chrono::Utc;
use serde_json::json;

#[test]
fn merges_parameters() {
    let defaults = json!({ "a": 1, "b": 2 });
    let overrides = json!({ "b": 3, "c": 4 });
    let merged = merge_parameters(&defaults, &overrides);
    assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
}

#[test]
fn builds_runtime_context() {
    let workflow_run = WorkflowRun {
        id: 10,
        workflow_id: 1,
        status: WorkflowStatus::Running,
        active_node_id: Some("curr".into()),
        parameters: json!({ "name": "foo" }),
        state: json!({ "loop_index": 0 }),
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
    };
    let node_runs = vec![WorkflowNodeRun {
        id: 100,
        workflow_run_id: 10,
        node_id: "prev".into(),
        task_run_id: None,
        status: WorkflowStatus::Succeeded,
        attempt: 1,
        parameters: json!({}),
        output_json: Some(json!({ "result": "ok" })),
        state: json!({}),
        transition_reason: None,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
    }];

    let ctx = runtime_context(&workflow_run, &node_runs);
    assert_eq!(ctx["input"]["name"], "foo");
    assert_eq!(ctx["steps"]["prev"]["output"]["result"], "ok");
    assert_eq!(ctx["workflow"]["run_id"], 10);
    assert_eq!(ctx["workflow"]["state"]["loop_index"], 0);
}
