//! request/response shapes the api promises: run-stream framing and the defaults a run request
//! falls back to when a field is omitted.

use super::*;

#[test]
fn workflow_run_stream_terminal_status_stays_snapshot_message() {
    let response = crate::models::WorkflowRunResponse {
        run: runinator_models::workflows::WorkflowRun {
            id: Uuid::now_v7(),
            workflow_id: Uuid::now_v7(),
            workflow_snapshot: None,
            status: runinator_models::workflows::WorkflowStatus::Succeeded,
            active_node_id: None,
            parameters: json!({}),
            state: json!({}),
            state_version: 0,
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: Some(chrono::Utc::now()),
            message: None,
            name: None,
            correlation_key: None,
            pipeline_run_id: None,
            trigger_source_kind: None,
            trigger_actor_type: None,
            trigger_actor_replica_id: None,
            trigger_actor_display_name: None,
            trigger_request_host: None,
            trigger_request_ip: None,
            trigger_metadata: Value::Null,
        },
        nodes: vec![],
    };

    let value: Value = serde_json::to_value(response).unwrap().into();

    assert_eq!(value["run"]["status"], "succeeded");
    assert_eq!(value["nodes"], json!([]));
    assert!(value.get("type").is_none());
}

#[test]
fn workflow_run_request_defaults_to_non_debug() {
    let request: crate::models::WorkflowRunRequest =
        serde_json::from_value(json!({ "parameters": { "mode": "test" } }).into()).unwrap();

    assert!(!request.debug);
    assert_eq!(request.parameters["mode"], "test");
}

#[test]
fn workflow_run_request_accepts_debug_flag() {
    let request: crate::models::WorkflowRunRequest =
        serde_json::from_value(json!({ "parameters": {}, "debug": true }).into()).unwrap();

    assert!(request.debug);
}
