use crate::context::*;
use crate::{api::WorkflowSchedulerApi, nodes::*};
use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
    runs::RunSummary,
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::json;
use std::sync::Mutex;

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

#[tokio::test]
async fn switch_routes_matching_default_and_unmatched_cases() {
    let run = workflow_run(json!({ "mode": "fast" }), json!({}), "route");
    let api = MockWorkflowApi::default();
    let route = node(json!({
        "id": "route",
        "kind": "switch",
        "parameters": {
            "value": { "$ref": { "input": ["mode"] } },
            "cases": [{ "equals": "fast", "target": { "$node": "fast_path" } }],
            "default": { "$node": "fallback" }
        }
    }));

    process_switch_node(&api, &run, &route, &[]).await.unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("fast_path")
    );

    let run = workflow_run(json!({ "mode": "slow" }), json!({}), "route");
    let api = MockWorkflowApi::default();
    process_switch_node(&api, &run, &route, &[]).await.unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("fallback")
    );

    let unmatched = node(json!({
        "id": "route",
        "kind": "switch",
        "parameters": {
            "value": { "$ref": { "input": ["mode"] } },
            "cases": [{ "equals": "fast", "target": { "$node": "fast_path" } }]
        }
    }));
    let api = MockWorkflowApi::default();
    process_switch_node(&api, &run, &unmatched, &[])
        .await
        .unwrap();
    assert_eq!(api.last_run_update().status, WorkflowStatus::Blocked);
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("route")
    );
}

#[tokio::test]
async fn emit_resolves_output_and_advances() {
    let run = workflow_run(json!({ "ticket": "RUN-7" }), json!({}), "emit");
    let api = MockWorkflowApi::default();
    let node = node(json!({
        "id": "emit",
        "kind": "emit",
        "parameters": {
            "event_type": "ticket.ready",
            "data": { "ticket": { "$ref": { "input": ["ticket"] } } }
        },
        "transitions": { "next": { "$node": "done" } }
    }));

    process_emit_node(&api, &run, &node, &[]).await.unwrap();

    let update = api.last_node_update();
    assert_eq!(update.status, WorkflowStatus::Succeeded);
    assert_eq!(update.output_json["event_type"], "ticket.ready");
    assert_eq!(update.output_json["data"]["ticket"], "RUN-7");
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn parallel_progresses_branches_into_join_all() {
    let parallel = node(json!({
        "id": "fanout",
        "kind": "parallel",
        "parameters": { "branches": [{ "$node": "a" }, { "$node": "b" }] }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "fanout");

    process_parallel_node(&api, &run, &parallel, None)
        .await
        .unwrap();
    let update = api.last_run_update();
    assert_eq!(update.active_node_id.as_deref(), Some("a"));
    assert_eq!(update.state["parallel"]["remaining"], json!(["b"]));

    let join = node(json!({
        "id": "join",
        "kind": "join",
        "parameters": { "wait_for": [{ "$node": "a" }, { "$node": "b" }], "mode": "all" },
        "transitions": { "next": { "$node": "done" } }
    }));
    let run = workflow_run(json!({}), update.state.clone(), "join");
    let api = MockWorkflowApi::default();
    process_join_node(
        &api,
        &run,
        &join,
        None,
        &[node_run("a", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(api.last_run_update().active_node_id.as_deref(), Some("b"));

    let run = workflow_run(json!({}), api.last_run_update().state.clone(), "join");
    let api = MockWorkflowApi::default();
    process_join_node(
        &api,
        &run,
        &join,
        None,
        &[
            node_run("a", WorkflowStatus::Succeeded),
            node_run("b", WorkflowStatus::Succeeded),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn join_any_succeeds_with_one_completed_branch() {
    let join = node(json!({
        "id": "join",
        "kind": "join",
        "parameters": { "wait_for": [{ "$node": "a" }, { "$node": "b" }], "mode": "any" },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "join");

    process_join_node(
        &api,
        &run,
        &join,
        None,
        &[node_run("a", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
}

#[tokio::test]
async fn try_routes_body_success_failure_and_finally() {
    let try_node = node(json!({
        "id": "guard",
        "kind": "try",
        "parameters": { "body": { "$node": "body" }, "catch": { "$node": "recover" }, "finally": { "$node": "cleanup" } },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "guard");

    process_try_node(&api, &run, &try_node, None, &[])
        .await
        .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("body")
    );

    let latest = node_run_with_id(1, "guard", WorkflowStatus::Running, None, json!({}));
    let run = workflow_run(
        json!({}),
        json!({ "try": { "node_id": "guard", "phase": "body" } }),
        "guard",
    );
    let api = MockWorkflowApi::default();
    process_try_node(
        &api,
        &run,
        &try_node,
        Some(&latest),
        &[node_run("body", WorkflowStatus::Failed)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("recover")
    );

    let run = workflow_run(
        json!({}),
        json!({ "try": { "node_id": "guard", "phase": "body" } }),
        "guard",
    );
    let api = MockWorkflowApi::default();
    process_try_node(
        &api,
        &run,
        &try_node,
        Some(&latest),
        &[node_run("body", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("cleanup")
    );
}

#[tokio::test]
async fn map_exposes_item_aggregates_output_and_propagates_failure() {
    let map = node(json!({
        "id": "batch",
        "kind": "map",
        "parameters": { "items": ["a", "b"], "target": { "$node": "item" }, "concurrency": 1 },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "batch");

    process_map_node(&api, &run, &map, None, &[]).await.unwrap();
    let update = api.last_run_update();
    assert_eq!(update.active_node_id.as_deref(), Some("item"));
    assert_eq!(update.state["map"]["item"], "a");
    assert_eq!(update.state["map"]["index"], 0);

    let latest = node_run_with_id(
        1,
        "batch",
        WorkflowStatus::Running,
        None,
        update.state["map"].clone(),
    );
    let run = workflow_run(json!({}), update.state.clone(), "batch");
    let api = MockWorkflowApi::default();
    process_map_node(
        &api,
        &run,
        &map,
        Some(&latest),
        &[node_run_with_output(
            "item",
            WorkflowStatus::Succeeded,
            json!({ "ok": "a" }),
        )],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("item")
    );
    assert_eq!(api.last_run_update().state["map"]["item"], "b");
    let second_state = api.last_run_update().state;

    let latest = node_run_with_id(
        1,
        "batch",
        WorkflowStatus::Running,
        None,
        second_state["map"].clone(),
    );
    let run = workflow_run(json!({}), second_state, "batch");
    let api = MockWorkflowApi::default();
    process_map_node(
        &api,
        &run,
        &map,
        Some(&latest),
        &[
            node_run_with_id(
                1,
                "item",
                WorkflowStatus::Succeeded,
                Some(json!({ "ok": "a" })),
                json!({}),
            ),
            node_run_with_id(
                2,
                "item",
                WorkflowStatus::Succeeded,
                Some(json!({ "ok": "b" })),
                json!({}),
            ),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
    assert_eq!(api.last_node_update().output_json["count"], 2);

    let failed_run = workflow_run(json!({}), update.state, "batch");
    let api = MockWorkflowApi::default();
    process_map_node(
        &api,
        &failed_run,
        &map,
        Some(&latest),
        &[node_run("item", WorkflowStatus::Failed)],
    )
    .await
    .unwrap();
    assert_eq!(api.last_run_update().status, WorkflowStatus::Failed);
}

#[tokio::test]
async fn race_records_first_success_and_starts_remaining_branches_sequentially() {
    let race = node(json!({
        "id": "race",
        "kind": "race",
        "parameters": { "branches": [{ "$node": "fast" }, { "$node": "slow" }], "winner": "first_success" },
        "transitions": { "next": { "$node": "done" } }
    }));
    let api = MockWorkflowApi::default();
    let run = workflow_run(json!({}), json!({}), "race");

    process_race_node(&api, &run, &race, None, &[])
        .await
        .unwrap();
    let update = api.last_run_update();
    assert_eq!(update.active_node_id.as_deref(), Some("fast"));
    assert_eq!(update.state["race"]["remaining"], json!(["slow"]));

    let latest = node_run_with_id(1, "race", WorkflowStatus::Running, None, json!({}));
    let run = workflow_run(json!({}), update.state, "race");
    let api = MockWorkflowApi::default();
    process_race_node(
        &api,
        &run,
        &race,
        Some(&latest),
        &[node_run("fast", WorkflowStatus::Succeeded)],
    )
    .await
    .unwrap();
    assert_eq!(
        api.last_run_update().active_node_id.as_deref(),
        Some("done")
    );
    assert_eq!(api.last_node_update().output_json["winner"], "fast");
}

#[derive(Debug, Clone)]
struct WorkflowRunUpdate {
    status: WorkflowStatus,
    active_node_id: Option<String>,
    state: serde_json::Value,
}

#[derive(Debug, Clone)]
struct WorkflowNodeRunUpdate {
    status: WorkflowStatus,
    output_json: serde_json::Value,
}

#[derive(Default)]
struct MockWorkflowApi {
    state: Mutex<MockWorkflowState>,
}

#[derive(Default)]
struct MockWorkflowState {
    next_node_run_id: i64,
    workflow_updates: Vec<WorkflowRunUpdate>,
    node_updates: Vec<WorkflowNodeRunUpdate>,
}

impl MockWorkflowApi {
    fn last_run_update(&self) -> WorkflowRunUpdate {
        self.state
            .lock()
            .unwrap()
            .workflow_updates
            .last()
            .cloned()
            .expect("workflow run update")
    }

    fn last_node_update(&self) -> WorkflowNodeRunUpdate {
        self.state
            .lock()
            .unwrap()
            .node_updates
            .last()
            .cloned()
            .expect("workflow node run update")
    }
}

#[async_trait]
impl WorkflowSchedulerApi for MockWorkflowApi {
    async fn fetch_tasks(&self) -> Result<Vec<ScheduledTask>, SendableError> {
        Ok(Vec::new())
    }

    async fn create_workflow_task_run(
        &self,
        _task_id: i64,
        _workflow_run_id: i64,
        _workflow_node_id: String,
        _parameters: serde_json::Value,
    ) -> Result<RunSummary, SendableError> {
        Err(test_error("unexpected task run creation"))
    }

    async fn fetch_run(&self, _run_id: i64) -> Result<RunSummary, SendableError> {
        Err(test_error("unexpected run fetch"))
    }

    async fn fetch_workflow(&self, _workflow_id: i64) -> Result<WorkflowDefinition, SendableError> {
        Err(test_error("unexpected workflow fetch"))
    }

    async fn create_workflow_run(
        &self,
        _workflow_id: i64,
        _parameters: serde_json::Value,
    ) -> Result<WorkflowRun, SendableError> {
        Err(test_error("unexpected workflow run creation"))
    }

    async fn fetch_workflow_runs_by_status(
        &self,
        _status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        Ok(Vec::new())
    }

    async fn update_workflow_run(
        &self,
        _workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<serde_json::Value>,
        _message: Option<String>,
    ) -> Result<(), SendableError> {
        self.state
            .lock()
            .unwrap()
            .workflow_updates
            .push(WorkflowRunUpdate {
                status,
                active_node_id,
                state: state.unwrap_or_else(|| json!({})),
            });
        Ok(())
    }

    async fn fetch_workflow_run(
        &self,
        _workflow_run_id: i64,
    ) -> Result<(WorkflowRun, Vec<WorkflowNodeRun>), SendableError> {
        Err(test_error("unexpected workflow run fetch"))
    }

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: &str,
        parameters: serde_json::Value,
    ) -> Result<WorkflowNodeRun, SendableError> {
        let mut state = self.state.lock().unwrap();
        state.next_node_run_id += 1;
        Ok(WorkflowNodeRun {
            id: state.next_node_run_id,
            workflow_run_id,
            node_id: node_id.into(),
            task_run_id: None,
            status: WorkflowStatus::Queued,
            attempt: 0,
            parameters,
            output_json: None,
            state: json!({}),
            transition_reason: None,
            created_at: Utc::now(),
            started_at: None,
            finished_at: None,
            message: None,
        })
    }

    async fn update_workflow_node_run(
        &self,
        _node_run_id: i64,
        status: WorkflowStatus,
        _task_run_id: Option<i64>,
        _attempt: Option<i64>,
        _parameters: Option<serde_json::Value>,
        output_json: Option<serde_json::Value>,
        _state: Option<serde_json::Value>,
        _transition_reason: Option<String>,
        _message: Option<String>,
    ) -> Result<(), SendableError> {
        self.state
            .lock()
            .unwrap()
            .node_updates
            .push(WorkflowNodeRunUpdate {
                status,
                output_json: output_json.unwrap_or(serde_json::Value::Null),
            });
        Ok(())
    }

    async fn create_automation_record(
        &self,
        _path: &str,
        _record: serde_json::Value,
    ) -> Result<serde_json::Value, SendableError> {
        Err(test_error("unexpected automation record creation"))
    }

    async fn fetch_idempotency_key(
        &self,
        _scope: &str,
        _key: &str,
    ) -> Result<Option<serde_json::Value>, SendableError> {
        Ok(None)
    }

    async fn put_idempotency_key(
        &self,
        _scope: &str,
        _key: &str,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, SendableError> {
        Ok(result)
    }
}

fn node(value: serde_json::Value) -> WorkflowNode {
    serde_json::from_value(value).unwrap()
}

fn workflow_run(
    parameters: serde_json::Value,
    state: serde_json::Value,
    active: &str,
) -> WorkflowRun {
    WorkflowRun {
        id: 10,
        workflow_id: 1,
        status: WorkflowStatus::Running,
        active_node_id: Some(active.into()),
        parameters,
        state,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
    }
}

fn node_run(node_id: &str, status: WorkflowStatus) -> WorkflowNodeRun {
    node_run_with_id(1, node_id, status, None, json!({}))
}

fn node_run_with_output(
    node_id: &str,
    status: WorkflowStatus,
    output: serde_json::Value,
) -> WorkflowNodeRun {
    node_run_with_id(1, node_id, status, Some(output), json!({}))
}

fn node_run_with_id(
    id: i64,
    node_id: &str,
    status: WorkflowStatus,
    output_json: Option<serde_json::Value>,
    state: serde_json::Value,
) -> WorkflowNodeRun {
    WorkflowNodeRun {
        id,
        workflow_run_id: 10,
        node_id: node_id.into(),
        task_run_id: None,
        status,
        attempt: 1,
        parameters: json!({}),
        output_json,
        state,
        transition_reason: None,
        created_at: Utc::now(),
        started_at: None,
        finished_at: None,
        message: None,
    }
}

fn test_error(message: &str) -> SendableError {
    Box::new(RuntimeError::new("scheduler.test".into(), message.into()))
}
