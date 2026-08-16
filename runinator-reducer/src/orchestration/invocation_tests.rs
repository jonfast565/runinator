//! the invocation node's lifecycle: pure completion, durable suspend, resume, and failure.
//!
//! these are the behavioural checks the plan called for, written here rather than against the web
//! service's sqlite suite because `FakeStore` reaches the handler directly. the one worth reading
//! first is `a_durable_call_suspends_and_resumes_under_one_node_run`: it is the whole premise of the
//! feature — the node run stays `Running` across a dispatch and settles only when the program does.

use super::*;
use chrono::Utc;
use runinator_models::invocation::{
    CallableTarget, InvocationInstruction, InvocationModule, InvocationProgram,
};
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::process_ready_node;
use crate::test_support::FakeStore;
use runinator_store::roles::InvocationStore;

const WORKFLOW_ID: &str = "22222222-2222-2222-2222-222222222222";

fn workflow(program: InvocationProgram) -> WorkflowDefinition {
    let module = InvocationModule::new(program);
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "invocation test",
        "version": "1.0.0",
        "enabled": true,
        "definition": {
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "invoke" } } },
                {
                    "id": "invoke",
                    "kind": "invocation",
                    "parameters": { "module": module },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }
    }))
    .expect("workflow definition")
}

fn run(run_id: Uuid) -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": run_id,
        "workflow_id": WORKFLOW_ID,
        "status": "queued",
        "active_node_id": null,
        "parameters": {},
        "state": {},
        "created_at": Utc::now(),
        "started_at": null,
        "finished_at": null,
        "message": null,
    }))
    .expect("workflow run")
}

fn ready_node(run_id: Uuid, node_id: &str) -> ReadyNodeRecord {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::now_v7(),
        "source_event_id": Uuid::now_v7(),
        "workflow_run_id": run_id,
        "node_id": node_id,
        "status": "queued",
        "ready_at": Utc::now(),
        "attempts": 0,
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
    }))
    .expect("ready node")
}

/// `return <const>` — the whole program folds in the reducer.
fn pure_program() -> InvocationProgram {
    InvocationProgram::new(vec![
        InvocationInstruction::Const {
            value: Value::from(7_i64),
        },
        InvocationInstruction::Return,
    ])
}

/// `return http.get()` — one durable call, whose result is the program's value.
fn durable_program() -> InvocationProgram {
    InvocationProgram::new(vec![
        InvocationInstruction::Call {
            target: CallableTarget::Provider {
                provider: "http".into(),
                function: "get".into(),
            },
            argc: 0,
            names: Vec::new(),
            policy: None,
        },
        InvocationInstruction::Return,
    ])
}

async fn start_run(program: InvocationProgram) -> (FakeStore, Uuid) {
    let store = FakeStore::new();
    let run_id = Uuid::now_v7();
    store.insert_workflow(workflow(program));
    store.insert_run(run(run_id));
    process_ready_node(&store, &ready_node(run_id, "start"))
        .await
        .expect("enter the invocation");
    (store, run_id)
}

#[tokio::test]
async fn a_pure_program_completes_without_dispatching() {
    let (store, run_id) = start_run(pure_program()).await;

    let node_run = store.latest_node_run("invoke").expect("invocation run");
    assert_eq!(node_run.status, WorkflowStatus::Succeeded);
    assert_eq!(node_run.output_json, Some(Value::from(7_i64)));
    // the point of folding in the reducer: no broker traffic at all.
    assert!(
        store.recorded_invocation_calls().is_empty(),
        "a pure program must not dispatch"
    );
    assert_eq!(
        store.run(run_id).expect("run").status,
        WorkflowStatus::Succeeded
    );
}

#[tokio::test]
async fn a_durable_call_suspends_and_resumes_under_one_node_run() {
    let (store, run_id) = start_run(durable_program()).await;

    // suspended: the node run is still running, and exactly one call was recorded.
    let node_run = store.latest_node_run("invoke").expect("invocation run");
    assert_eq!(node_run.status, WorkflowStatus::Running);
    let calls = store.recorded_invocation_calls();
    assert_eq!(calls.len(), 1, "one durable effect, one call record");
    assert_eq!(
        calls[0].target,
        CallableTarget::Provider {
            provider: "http".into(),
            function: "get".into()
        }
    );
    assert_eq!(calls[0].sequence, 0);

    // the call lands, and the next drive resumes the program with its value.
    store
        .settle_invocation_call(
            calls[0].id,
            0,
            WorkflowStatus::Succeeded,
            Some(Value::from("ok")),
            None,
        )
        .await
        .expect("settle the call");
    process_ready_node(&store, &ready_node(run_id, "invoke"))
        .await
        .expect("resume the invocation");

    let node_run = store.latest_node_run("invoke").expect("invocation run");
    assert_eq!(node_run.status, WorkflowStatus::Succeeded);
    assert_eq!(node_run.output_json, Some(Value::from("ok")));
    // still one node run and one call: the whole point is that the call did not create its own.
    assert_eq!(store.recorded_invocation_calls().len(), 1);
    assert_eq!(
        store.run(run_id).expect("run").status,
        WorkflowStatus::Succeeded
    );
}

#[tokio::test]
async fn a_failed_call_fails_the_invocation() {
    let (store, run_id) = start_run(durable_program()).await;
    let calls = store.recorded_invocation_calls();

    store
        .settle_invocation_call(
            calls[0].id,
            0,
            WorkflowStatus::Failed,
            None,
            Some("upstream refused".into()),
        )
        .await
        .expect("settle the call");
    process_ready_node(&store, &ready_node(run_id, "invoke"))
        .await
        .expect("resume the invocation");

    let node_run = store.latest_node_run("invoke").expect("invocation run");
    assert_eq!(node_run.status, WorkflowStatus::Failed);
    assert_eq!(node_run.message.as_deref(), Some("upstream refused"));
    let invocation = store.invocation_for(node_run.id).expect("invocation");
    assert_eq!(invocation.status, WorkflowStatus::Failed);
}

#[tokio::test]
async fn a_redriven_suspend_does_not_dispatch_twice() {
    let (store, run_id) = start_run(durable_program()).await;
    assert_eq!(store.recorded_invocation_calls().len(), 1);

    // a duplicate drive while the call is still in flight must leave it alone: the program has not
    // moved, so re-stepping it would reach the same call, and the sequence is what stops a second
    // dispatch.
    process_ready_node(&store, &ready_node(run_id, "invoke"))
        .await
        .expect("re-drive while parked");
    assert_eq!(
        store.recorded_invocation_calls().len(),
        1,
        "a re-drive must not dispatch a second time"
    );
    assert_eq!(
        store.latest_node_run("invoke").expect("run").status,
        WorkflowStatus::Running
    );
}
