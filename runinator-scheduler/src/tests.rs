use crate::context::*;
use chrono::{TimeZone, Utc};
use runinator_models::workflows::{WorkflowNodeRun, WorkflowRun, WorkflowStatus};
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
    assert_eq!(ctx["prev"]["result"], "ok");
    assert_eq!(ctx["workflow"]["run_id"], 10);
    assert_eq!(ctx["workflow"]["state"]["loop_index"], 0);
}

#[test]
fn initializes_next_execution_from_cron_schedule() {
    let now = Utc.with_ymd_and_hms(2026, 5, 9, 2, 30, 0).unwrap();
    let next = crate::db_extensions::next_execution_for_cron("0 0,9,12,15,18,21 * * *", now)
        .expect("cron schedule is valid");

    assert_eq!(next, Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap());
}
