//! interrupt handlers driven end to end against the in-memory store.
//!
//! the shape under test is: a `wait` whose deadline elapses raises the `wake` interrupt, a handler
//! region runs on a second cursor, and a `resume` node hands control back. each test names the
//! behaviour it protects, because most of these are rules that fail *silently* when broken — a
//! stalled cursor, a re-raised interrupt, a double-dispatched action.

use chrono::{Duration, Utc};
use runinator_models::interrupt::{InterruptSource, PendingInterrupt};
use runinator_models::orchestration::ReadyNodeRecord;
use runinator_models::value::Value;

use runinator_models::workflow_state::WorkflowRunState;
use runinator_models::workflows::{WorkflowDefinition, WorkflowRun, WorkflowStatus};
use uuid::Uuid;

use crate::test_support::FakeStore;
use crate::{ReadyNodeDisposition, process_ready_node};

const WORKFLOW_ID: &str = "11111111-1111-1111-1111-111111111111";
const RUN_ID: &str = "22222222-2222-2222-2222-222222222222";

fn workflow_with(nodes: serde_json::Value, interrupts: serde_json::Value) -> WorkflowDefinition {
    serde_json::from_value(serde_json::json!({
        "id": WORKFLOW_ID,
        "name": "interrupt test",
        "version": "1.0.0",
        "enabled": true,
        "definition": {
            "start": "start",
            "nodes": nodes,
            "metadata": { "interrupts": interrupts },
        }
    }))
    .expect("workflow definition")
}

fn queued_run() -> WorkflowRun {
    serde_json::from_value(serde_json::json!({
        "id": RUN_ID,
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

fn ready_node(node_id: &str) -> ReadyNodeRecord {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::now_v7(),
        "source_event_id": Uuid::now_v7(),
        "workflow_run_id": RUN_ID,
        "node_id": node_id,
        "status": "queued",
        "ready_at": Utc::now(),
        "attempts": 0,
        "created_at": Utc::now(),
        "updated_at": Utc::now(),
    }))
    .expect("ready node")
}

/// the main flow: start → poll (a zero-second wait, so its deadline is already up on the next
/// drive) → end. `body` is spliced in as the handler region.
fn main_flow(body: serde_json::Value) -> serde_json::Value {
    let mut nodes = serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "poll" } } },
        {
            "id": "poll", "kind": "wait", "wait": { "seconds": 0 },
            "transitions": { "next": { "$node": "end" } }
        },
        { "id": "end", "kind": "end" }
    ]);
    let list = nodes.as_array_mut().expect("array");
    for node in body.as_array().expect("body is an array") {
        list.push(node.clone());
    }
    nodes
}

/// a region of one transform, terminated by a `resume` in `mode`.
fn region(mode: &str) -> serde_json::Value {
    serde_json::json!([
        {
            "id": "refresh", "kind": "transform",
            "parameters": { "bindings": { "touched": true } },
            "transitions": { "on_success": { "$node": "handled" } }
        },
        { "id": "handled", "kind": "resume", "parameters": { "mode": mode } }
    ])
}

fn wake_handler() -> serde_json::Value {
    serde_json::json!([{ "on": "wake", "handler": "refresh" }])
}

fn state(store: &FakeStore) -> WorkflowRunState {
    let run = store.run(RUN_ID.parse().expect("run id")).expect("run");
    run.execution_state
}

/// park the wait, then drive it again so its elapsed deadline raises the interrupt.
/// returns the ready row the handler was armed with.
async fn park_then_raise(store: &FakeStore) -> ReadyNodeRecord {
    process_ready_node(store, &ready_node("poll"))
        .await
        .expect("first drive parks the wait");
    let parked = store.latest_node_run("poll").expect("the wait parked");
    assert_eq!(parked.status, WorkflowStatus::Waiting);

    process_ready_node(store, &ready_node("poll"))
        .await
        .expect("second drive raises the interrupt");

    store
        .ready_nodes()
        .into_iter()
        .find(|row| row.node_id == "refresh" && row.completed_at.is_none())
        .expect("raising an interrupt arms the handler region's entry")
}

#[tokio::test]
async fn an_elapsed_wait_raises_the_wake_interrupt_and_suspends_the_thread() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(main_flow(region("resume")), wake_handler()));
    store.insert_run(queued_run());

    let armed = park_then_raise(&store).await;

    let state = state(&store);
    assert_eq!(state.cursors.len(), 2, "the thread plus its handler");
    let handler = state
        .cursors
        .iter()
        .find(|cursor| cursor.is_interrupt_handler())
        .expect("a handler cursor was forked");
    assert_eq!(handler.node_id(), "refresh");
    assert_eq!(
        armed.cursor_id,
        Some(handler.id),
        "the ready row must name the handler cursor, or the drive resolves the wrong thread"
    );

    let interrupted = state
        .cursors
        .iter()
        .find(|cursor| !cursor.is_interrupt_handler())
        .expect("the interrupted thread is still present");
    assert!(interrupted.is_suspended());
    assert_eq!(interrupted.node_id(), "poll");

    assert_eq!(
        store.latest_node_run("poll").expect("node run").status,
        WorkflowStatus::Waiting,
        "raising an interrupt must not settle the node the thread was parked on"
    );
}

#[tokio::test]
async fn a_drive_for_a_suspended_cursor_does_nothing() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(main_flow(region("resume")), wake_handler()));
    store.insert_run(queued_run());
    park_then_raise(&store).await;

    let before = store.node_runs().len();
    let disposition = process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("a stray wake for the frozen thread");

    assert_eq!(disposition, ReadyNodeDisposition::Complete);
    assert_eq!(
        store.node_runs().len(),
        before,
        "a suspended cursor must not process its node; armed wakes landing here are dropped"
    );
    assert!(
        state(&store)
            .cursors
            .iter()
            .any(|cursor| cursor.is_suspended()),
        "the thread stays frozen until the handler returns control"
    );
}

/// the headline behaviour: control comes back to the node the thread was on, and — critically —
/// the interrupt does not immediately raise itself again, even though the wait deadline is still
/// elapsed. that is what the per-cursor `handled` record buys.
#[tokio::test]
async fn resume_returns_control_and_does_not_re_raise() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(main_flow(region("resume")), wake_handler()));
    store.insert_run(queued_run());
    let armed = park_then_raise(&store).await;

    // the drive follows the thread past the handoff: the handler retires, the thread resumes on its
    // wait, and — the deadline having elapsed — that wait completes and the run finishes, all inside
    // this one drive.
    process_ready_node(&store, &armed)
        .await
        .expect("the handler region runs to its resume node and control returns");

    assert!(
        store.latest_node_run("refresh").is_some(),
        "the region actually executed"
    );
    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(
        run.active_node_id.as_deref(),
        Some("end"),
        "control returned to the wait, whose elapsed deadline then completed it normally"
    );
    assert_eq!(
        run.status,
        WorkflowStatus::Succeeded,
        "an interrupt is transparent to the run's outcome"
    );
    assert_eq!(
        store
            .node_runs()
            .iter()
            .filter(|run| run.node_id == "refresh")
            .count(),
        1,
        "the interrupt must fire exactly once. the wait deadline is *still* elapsed when control \
         comes back, so without the per-cursor `handled` record this re-raises forever"
    );
    assert!(
        state(&store)
            .cursors
            .iter()
            .all(|cursor| !cursor.is_interrupt_handler()),
        "no handler cursor outlives its region"
    );
}

#[tokio::test]
async fn resume_next_skips_past_the_interrupted_node() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(main_flow(region("continue")), wake_handler()));
    store.insert_run(queued_run());
    let armed = park_then_raise(&store).await;

    process_ready_node(&store, &armed).await.expect("handler");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(
        run.active_node_id.as_deref(),
        Some("end"),
        "`resume next` takes the interrupted node's success edge"
    );
    let poll = store.latest_node_run("poll").expect("node run");
    assert!(
        poll.status.is_terminal(),
        "leaving a non-terminal run behind would hang the thread on its next visit, since \
         latest_node_run would keep returning it: got {:?}",
        poll.status
    );
}

/// failing *inside* an interrupt is not failing the workflow. the handler is a side-channel, so its
/// failure retires the handler and returns control — the thread it suspended is still valid work.
#[tokio::test]
async fn resume_fail_routes_the_interrupted_node_without_failing_the_run() {
    let store = FakeStore::new();
    let nodes = main_flow(serde_json::json!([
        {
            "id": "refresh", "kind": "transform",
            "parameters": { "bindings": { "touched": true } },
            "transitions": { "on_success": { "$node": "handled" } }
        },
        { "id": "handled", "kind": "resume", "parameters": { "mode": "fail" } }
    ]));
    // give the interrupted node somewhere to go on failure, so the run continues rather than ending.
    let nodes = with_failure_edge(nodes);
    store.insert_workflow(workflow_with(nodes, wake_handler()));
    store.insert_run(queued_run());
    let armed = park_then_raise(&store).await;

    process_ready_node(&store, &armed).await.expect("handler");

    assert!(
        store.latest_node_run("recover").is_some(),
        "`resume fail` settles the interrupted node Failed and takes its on_failure edge"
    );
    assert_eq!(
        store.latest_node_run("poll").expect("node run").status,
        WorkflowStatus::Failed,
        "the interrupted node is the thing that failed"
    );
    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_ne!(
        run.status,
        WorkflowStatus::Failed,
        "failing inside an interrupt is not failing the workflow: the main flow routed the \
         failure through its own on_failure edge and carried on"
    );
}

/// add an `on_failure` edge to `poll` and a `recover` node for it to land on.
fn with_failure_edge(mut nodes: serde_json::Value) -> serde_json::Value {
    let list = nodes.as_array_mut().expect("array");
    for node in list.iter_mut() {
        if node["id"] == "poll" {
            node["transitions"]["on_failure"] = serde_json::json!({ "$node": "recover" });
        }
    }
    list.push(serde_json::json!({
        "id": "recover", "kind": "transform",
        "parameters": { "bindings": {} },
        "transitions": { "on_success": { "$node": "end" } }
    }));
    nodes
}

/// a handler whose region dies with nowhere to route must still hand control back. otherwise the
/// suspended cursor is frozen with no handler alive to release it, and the run hangs forever.
#[tokio::test]
async fn a_failing_handler_releases_the_thread_it_suspended() {
    let store = FakeStore::new();
    // an assert that cannot hold: fails, and the region declares no on_failure route.
    let nodes = main_flow(serde_json::json!([
        {
            "id": "refresh", "kind": "assert",
            "parameters": { "assertions": [{ "name": "impossible", "condition": false }] },
            "transitions": { "on_success": { "$node": "handled" } }
        },
        { "id": "handled", "kind": "resume" }
    ]));
    store.insert_workflow(workflow_with(nodes, wake_handler()));
    store.insert_run(queued_run());
    let armed = park_then_raise(&store).await;

    process_ready_node(&store, &armed)
        .await
        .expect("the handler fails");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_ne!(
        run.status,
        WorkflowStatus::Failed,
        "a handler is a side-channel; its failure must not take the run with it"
    );
    // control came back and the thread carried on to its own ending, which is the proof it was
    // released — a thread left frozen would have stalled on `poll` forever instead.
    assert_eq!(
        run.active_node_id.as_deref(),
        Some("end"),
        "the suspended thread resumed and completed despite the handler dying"
    );
    assert!(
        state(&store)
            .cursors
            .iter()
            .all(|cursor| !cursor.is_suspended() && !cursor.is_interrupt_handler()),
        "a frozen cursor with no handler left to free it is a permanent stall"
    );
}

#[tokio::test]
async fn no_declared_handler_leaves_the_drive_untouched() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(
        main_flow(region("resume")),
        serde_json::json!([]),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("park");
    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("elapse");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(
        run.active_node_id.as_deref(),
        Some("end"),
        "with no handler declared the wait completes exactly as it always did"
    );
    assert_eq!(
        run.status,
        WorkflowStatus::Succeeded,
        "the feature is invisible to a workflow that does not use it"
    );
}

#[tokio::test]
async fn a_disabled_handler_leaves_the_drive_untouched() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(
        main_flow(region("resume")),
        serde_json::json!([{ "on": "wake", "handler": "refresh", "enabled": false }]),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("park");
    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("elapse");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(run.active_node_id.as_deref(), Some("end"));
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    assert!(
        state(&store)
            .cursors
            .iter()
            .all(|cursor| !cursor.is_interrupt_handler()),
        "a disabled metadata link must never fork a handler cursor"
    );
}

/// a region built from kinds the handler allowlist excludes never reaches the runtime at all:
/// validation is the primary gate, and the runtime re-validates the definition on every drive.
///
/// the runtime's own `interrupt_region_is_supported` check is not redundant with this — it is what
/// covers a definition written by a binary whose allowlist differs from this one — but it is
/// unreachable from a definition this binary would accept, which is why it is asserted through the
/// shared predicate rather than through a drive.
#[tokio::test]
async fn a_region_of_unsupported_kinds_never_runs() {
    let store = FakeStore::new();
    // `signal` parks, and a parked handler would pin the suspended thread open indefinitely.
    let nodes = main_flow(serde_json::json!([
        {
            "id": "refresh", "kind": "signal",
            "parameters": { "name": "late" },
            "transitions": { "on_success": { "$node": "handled" } }
        },
        { "id": "handled", "kind": "resume" }
    ]));
    let workflow = workflow_with(nodes, wake_handler());
    let (_, parsed) = runinator_workflows::parse_nodes(&workflow).expect("the graph itself parses");
    assert!(
        !runinator_workflows::interrupt_region_is_supported("refresh", &parsed),
        "the runtime guard must agree with the validator about this region"
    );

    store.insert_workflow(workflow);
    store.insert_run(queued_run());
    assert!(
        process_ready_node(&store, &ready_node("poll"))
            .await
            .is_err(),
        "validation refuses the definition rather than letting an unrunnable handler be armed"
    );
}

/// an unknown source is ignored rather than rejected, so a definition written against a newer
/// binary still runs on this one.
#[tokio::test]
async fn a_handler_for_an_unknown_source_never_fires() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(
        main_flow(region("resume")),
        serde_json::json!([{ "on": "webhook", "handler": "refresh" }]),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("park");
    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("elapse");

    assert_eq!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("end")
    );
}

/// reaching a `resume` outside a handler region is a definition error, not a crash. the runtime
/// guard exists because the grammar cannot express "only inside a region" — that is reachability.
#[tokio::test]
async fn a_resume_node_outside_a_handler_blocks() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(
        serde_json::json!([
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "stray" } } },
            { "id": "stray", "kind": "resume" },
            { "id": "end", "kind": "end" }
        ]),
        serde_json::json!([]),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("stray"))
        .await
        .expect("drive");

    let run = store.run(RUN_ID.parse().unwrap()).expect("run");
    assert_eq!(
        run.status,
        WorkflowStatus::Blocked,
        "a resume with no interrupt to finish blocks rather than silently retiring the thread"
    );
}

/// the interrupt fires when the wait is actually up, not merely because a drive arrived.
#[tokio::test]
async fn a_wait_still_inside_its_window_does_not_raise() {
    let store = FakeStore::new();
    let nodes = main_flow(region("resume"));
    let nodes = {
        let mut nodes = nodes;
        for node in nodes.as_array_mut().unwrap() {
            if node["id"] == "poll" {
                node["wait"]["seconds"] = serde_json::json!(3600);
            }
        }
        nodes
    };
    store.insert_workflow(workflow_with(nodes, wake_handler()));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("park");
    let disposition = process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("early drive");

    assert_eq!(disposition, ReadyNodeDisposition::KeepClaim);
    assert_eq!(
        state(&store).cursors.len(),
        1,
        "no handler is forked while the wait still has time to run"
    );
}

/// `age_node_run` is how the other suites move time; make sure the wake source agrees with the
/// wait handler about when a deadline has passed, since they are two readers of one fact.
#[tokio::test]
async fn the_wake_source_and_the_wait_handler_agree_on_the_deadline() {
    let store = FakeStore::new();
    let mut nodes = main_flow(region("resume"));
    for node in nodes.as_array_mut().unwrap() {
        if node["id"] == "poll" {
            node["wait"]["seconds"] = serde_json::json!(60);
        }
    }
    store.insert_workflow(workflow_with(nodes, wake_handler()));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("park");
    let parked = store.latest_node_run("poll").expect("node run");
    // the deadline lives in the node-run state as an absolute timestamp, so aging the row is not
    // enough on its own — this asserts the two readers still line up once it genuinely elapses.
    store.age_node_run(parked.id, Duration::seconds(120));

    process_ready_node(&store, &ready_node("poll"))
        .await
        .expect("drive");

    let state = state(&store);
    let raised = state.cursors.iter().any(|c| c.is_interrupt_handler());
    let completed = store
        .latest_node_run("poll")
        .is_some_and(|run| run.status == WorkflowStatus::Succeeded);
    assert!(
        raised || !completed,
        "if the wait handler thinks the deadline elapsed, the wake source must too — otherwise \
         the interrupt is silently skipped exactly when it is supposed to fire"
    );
}

/// a handler's work must not leak into the resumed thread's `steps.*`. two layers stop it.
///
/// the primary gate is region validation, which refuses to *author* the read at all — proven here,
/// because the runtime re-validates the definition on every drive, so this shape can never run.
#[tokio::test]
async fn the_main_flow_cannot_author_a_read_of_a_handlers_output() {
    let store = FakeStore::new();
    let nodes = main_flow(region("continue"));
    let nodes = {
        let mut nodes = nodes;
        let list = nodes.as_array_mut().expect("array");
        for node in list.iter_mut() {
            if node["id"] == "poll" {
                node["transitions"]["next"] = serde_json::json!({ "$node": "check" });
            }
        }
        list.push(serde_json::json!({
            "id": "check", "kind": "transform",
            "parameters": { "bindings": { "seen": { "$ref": { "node": "refresh", "output": [] } } } },
            "transitions": { "on_success": { "$node": "end" } }
        }));
        nodes
    };
    store.insert_workflow(workflow_with(nodes, wake_handler()));
    store.insert_run(queued_run());

    let error = process_ready_node(&store, &ready_node("poll"))
        .await
        .expect_err("reading a region output from outside is not a valid workflow");
    assert!(
        error.to_string().contains("reads region output"),
        "the main flow must not be able to name a handler's step: {error}"
    );
}

/// the second layer: even for a definition that got past validation, the run context a non-handler
/// thread sees carries none of the region's node runs.
///
/// this is keyed on the region's *nodes* rather than on a live cursor, and that is the point — a
/// handler cursor retires as soon as its region ends, so a cursor-keyed test would let the region's
/// output reappear the instant control returned. asserted directly because validation makes the
/// end-to-end shape unauthorable.
#[test]
fn a_resumed_thread_sees_no_region_node_runs_even_after_the_handler_retires() {
    use runinator_models::cursor::RunCursor;
    use runinator_models::workflow_state::WorkflowRunState;
    use std::collections::HashSet;

    let region_nodes: HashSet<String> = ["refresh", "handled"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let node_runs = vec![node_run_for("poll"), node_run_for("refresh")];

    // the handler cursor is gone; only the resumed thread remains.
    let state = WorkflowRunState::default();
    let resumed = RunCursor::at("poll");
    let visible = super::context::visible_node_runs(&resumed, &state, &node_runs, &region_nodes);
    assert_eq!(
        visible
            .iter()
            .map(|run| run.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["poll"],
        "the region's work stays hidden after its cursor retired"
    );

    // the handler itself does see its own region, or it could not read its own steps.
    let handler = RunCursor::interrupt_handler(
        "refresh",
        runinator_models::interrupt::InterruptFrame::default(),
    );
    let visible = super::context::visible_node_runs(&handler, &state, &node_runs, &region_nodes);
    assert_eq!(
        visible.len(),
        2,
        "a handler reads both the run's context and its own"
    );
}

fn node_run_for(node_id: &str) -> runinator_models::workflows::WorkflowNodeRun {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::now_v7(),
        "workflow_run_id": RUN_ID,
        "node_id": node_id,
        "status": "succeeded",
        "attempt": 1,
        "parameters": {},
        "state": {},
        "created_at": Utc::now(),
        "message": null,
    }))
    .expect("node run")
}

/// the converse: the region can read the main thread's context, which is what makes sharing the run
/// worth anything at all.
#[tokio::test]
async fn the_handler_can_read_the_interrupted_thread_context() {
    let store = FakeStore::new();
    let nodes = main_flow(serde_json::json!([
        {
            "id": "refresh", "kind": "transform",
            "parameters": { "bindings": { "source": { "$ref": { "interrupt": ["source"] } } } },
            "transitions": { "on_success": { "$node": "handled" } }
        },
        { "id": "handled", "kind": "resume" }
    ]));
    store.insert_workflow(workflow_with(nodes, wake_handler()));
    store.insert_run(queued_run());
    let armed = park_then_raise(&store).await;

    process_ready_node(&store, &armed).await.expect("handler");

    let refresh = store.latest_node_run("refresh").expect("region ran");
    let source = refresh
        .output_json
        .as_ref()
        .and_then(|output| output.get("bindings"))
        .and_then(|bindings| bindings.get("source"))
        .and_then(|value| value.as_str());
    assert_eq!(
        source,
        Some("wake"),
        "the region reads what raised it under `interrupt.*`"
    );
}

/// time spent frozen behind an interrupt is not charged to the park it interrupted.
///
/// a parked node's deadline runs from its node run's `created_at`, so before the suspension credit a
/// handler that took longer than the remaining window would make the park time out the instant
/// control came back — the run would fail on a deadline it never actually waited out.
#[tokio::test]
async fn a_slow_handler_does_not_consume_the_parks_timeout() {
    let store = FakeStore::new();
    // a signal parked behind a 60s timeout, sitting after the wait that raises the interrupt.
    let nodes = main_flow(region("continue"));
    let nodes = {
        let mut nodes = nodes;
        let list = nodes.as_array_mut().expect("array");
        for node in list.iter_mut() {
            if node["id"] == "poll" {
                node["transitions"]["next"] = serde_json::json!({ "$node": "hold" });
            }
        }
        list.push(serde_json::json!({
            "id": "hold", "kind": "signal",
            "parameters": { "name": "later" },
            "timeout_seconds": 60,
            "transitions": {
                "on_success": { "$node": "end" },
                "on_timeout": { "$node": "gave_up" }
            }
        }));
        list.push(serde_json::json!({ "id": "gave_up", "kind": "end" }));
        nodes
    };
    store.insert_workflow(workflow_with(nodes, wake_handler()));
    store.insert_run(queued_run());

    // run the interrupt, which leaves the thread parked on `hold`.
    let armed = park_then_raise(&store).await;
    process_ready_node(&store, &armed).await.expect("handler");
    let parked = store.latest_node_run("hold").expect("the signal parked");
    assert_eq!(parked.status, WorkflowStatus::Waiting);

    // credit the cursor as though the handler had run for two minutes, then age the park to match.
    // without the credit this is 120s into a 60s window and the signal times out.
    super::run_state::mutate_cursor(
        &store,
        RUN_ID.parse().unwrap(),
        state(&store)
            .primary_cursor()
            .expect("the resumed thread")
            .id,
        |cursor| cursor.suspended_seconds = 120,
    )
    .await
    .expect("credit the suspension");
    store.age_node_run(parked.id, Duration::seconds(90));

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive the parked signal");

    let still_parked = store.latest_node_run("hold").expect("node run");
    assert_eq!(
        still_parked.status,
        WorkflowStatus::Waiting,
        "90s of a 60s window is inside the deadline once 120s of handler time is credited back"
    );
    assert_ne!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("gave_up"),
        "the park must not fail on a deadline it never actually waited out"
    );
}

/// the credit is not a blanket reprieve: a park that genuinely outlives its window still times out.
#[tokio::test]
async fn the_suspension_credit_does_not_disable_the_timeout() {
    let store = FakeStore::new();
    store.insert_workflow(workflow(serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "hold" } } },
        {
            "id": "hold", "kind": "signal",
            "parameters": { "name": "later" },
            "timeout_seconds": 60,
            "transitions": {
                "on_success": { "$node": "end" },
                "on_timeout": { "$node": "gave_up" }
            }
        },
        { "id": "gave_up", "kind": "end" },
        { "id": "end", "kind": "end" }
    ])));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    let parked = store.latest_node_run("hold").expect("node run");
    super::run_state::mutate_cursor(
        &store,
        RUN_ID.parse().unwrap(),
        state(&store).primary_cursor().expect("cursor").id,
        |cursor| cursor.suspended_seconds = 30,
    )
    .await
    .expect("credit");
    // 200s elapsed against a 60s window plus 30s of credit: still comfortably over.
    store.age_node_run(parked.id, Duration::seconds(200));

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive");

    assert_eq!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("gave_up"),
        "a park that really did outlive its window still follows on_timeout"
    );
}

/// a plain workflow fixture with no interrupts declared, for the timeout tests above.
fn workflow(nodes: serde_json::Value) -> WorkflowDefinition {
    workflow_with(nodes, serde_json::json!([]))
}

// --- the other sources -------------------------------------------------------
//
// each source is a predicate over the node state a drive finds, so each test below builds that
// state the way the runtime does — park it, land a result, schedule a retry — rather than writing
// the node run by hand. that is what keeps a source honest about the shape it claims to match.

fn handler_for(source: &str) -> serde_json::Value {
    serde_json::json!([{ "on": source, "handler": "refresh" }])
}

/// start → hold (whatever `node` is) → end, plus a `resume`-terminated region.
fn flow_around(node: serde_json::Value, mode: &str) -> serde_json::Value {
    let mut nodes = serde_json::json!([
        { "id": "start", "kind": "start", "transitions": { "next": { "$node": "hold" } } },
        { "id": "end", "kind": "end" },
        { "id": "gave_up", "kind": "end" },
    ]);
    let list = nodes.as_array_mut().expect("array");
    list.push(node);
    for entry in region(mode).as_array().expect("array") {
        list.push(entry.clone());
    }
    nodes
}

/// the handler cursor's armed ready row, or `None` when nothing was raised.
fn armed_handler(store: &FakeStore) -> Option<ReadyNodeRecord> {
    store
        .ready_nodes()
        .into_iter()
        .find(|row| row.node_id == "refresh" && row.completed_at.is_none())
}

fn raised(store: &FakeStore) -> bool {
    state(store)
        .cursors
        .iter()
        .any(|cursor| cursor.is_interrupt_handler())
}

/// an action node whose worker result the test lands by hand, the way the engine's result loop does.
fn action_node(extra: serde_json::Value) -> serde_json::Value {
    let mut node = serde_json::json!({
        "id": "hold", "kind": "action",
        "action": { "provider": "test", "function": "ship_it", "configuration": {} },
        "transitions": {
            "on_success": { "$node": "end" },
            "on_failure": { "$node": "gave_up" }
        }
    });
    for (key, value) in extra.as_object().expect("object") {
        node[key] = value.clone();
    }
    node
}

/// `timeout` fires *before* the node times out, which is the whole point: the handler still has a
/// live node run to decide about, so it can extend the window instead of only reacting to a failure.
#[tokio::test]
async fn a_blown_deadline_raises_timeout_before_the_node_gives_up() {
    let store = FakeStore::new();
    let hold = serde_json::json!({
        "id": "hold", "kind": "signal",
        "parameters": { "name": "later" },
        "timeout_seconds": 60,
        "transitions": {
            "on_success": { "$node": "end" },
            "on_timeout": { "$node": "gave_up" }
        }
    });
    store.insert_workflow(workflow_with(
        flow_around(hold, "restart"),
        handler_for("timeout"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    let parked = store.latest_node_run("hold").expect("the signal parked");
    store.age_node_run(parked.id, Duration::seconds(120));

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("the blown deadline raises the interrupt");

    assert!(
        raised(&store),
        "120s into a 60s window must raise `timeout`"
    );
    assert_eq!(
        store.latest_node_run("hold").expect("node run").status,
        WorkflowStatus::Waiting,
        "the node must not have been timed out yet — the handler decides what happens to it"
    );

    // `resume restart` cancels the overrun run and re-enters the node fresh, which is how a handler
    // buys the park another window.
    process_ready_node(&store, &armed_handler(&store).expect("armed"))
        .await
        .expect("handler");

    let runs: Vec<_> = store
        .node_runs()
        .into_iter()
        .filter(|run| run.node_id == "hold")
        .collect();
    assert_eq!(runs.len(), 2, "restart re-enters the node with a fresh run");
    assert_eq!(
        store.latest_node_run("hold").expect("node run").status,
        WorkflowStatus::Waiting,
        "the new park starts its clock over rather than inheriting a blown deadline"
    );
    assert_ne!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("gave_up"),
        "the handler extended the window; the node never took its on_timeout edge"
    );
}

/// the same node with no `timeout` handler declared must time out exactly as it always did.
#[tokio::test]
async fn a_blown_deadline_without_a_handler_still_times_out() {
    let store = FakeStore::new();
    let hold = serde_json::json!({
        "id": "hold", "kind": "signal",
        "parameters": { "name": "later" },
        "timeout_seconds": 60,
        "transitions": {
            "on_success": { "$node": "end" },
            "on_timeout": { "$node": "gave_up" }
        }
    });
    store.insert_workflow(workflow_with(
        flow_around(hold, "resume"),
        serde_json::json!([]),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    let parked = store.latest_node_run("hold").expect("node run");
    store.age_node_run(parked.id, Duration::seconds(120));
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive");

    assert_eq!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("gave_up"),
        "the feature stays invisible to a workflow that declared no handler for it"
    );
}

/// a deadline the author never wrote down does not raise `timeout`. the parks that fall back to an
/// implicit default do so to stop a run hanging forever, which is not an authoring decision the
/// handler should be reacting to.
#[tokio::test]
async fn a_node_with_no_declared_timeout_never_raises_timeout() {
    let store = FakeStore::new();
    let hold = serde_json::json!({
        "id": "hold", "kind": "signal",
        "parameters": { "name": "later" },
        "transitions": { "on_success": { "$node": "end" } }
    });
    store.insert_workflow(workflow_with(
        flow_around(hold, "resume"),
        handler_for("timeout"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    let parked = store.latest_node_run("hold").expect("node run");
    store.age_node_run(parked.id, Duration::days(365));
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive");

    assert!(!raised(&store), "no declared deadline, nothing to overrun");
}

/// `failure` is the error-handler source: a worker result lands `Failed` and the handler runs before
/// the thread takes its failure route. `resume next` is what lets a handler swallow the failure.
#[tokio::test]
async fn a_landed_failure_raises_the_failure_interrupt() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(
        flow_around(action_node(serde_json::json!({})), "continue"),
        handler_for("failure"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("dispatch");
    let dispatched = store
        .latest_node_run("hold")
        .expect("the action dispatched");
    assert_eq!(dispatched.status, WorkflowStatus::Running);
    store.resolve_node_run(dispatched.id, WorkflowStatus::Failed, None);

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("the landed failure raises the interrupt");
    assert!(raised(&store));
    assert_eq!(
        store.dispatches().len(),
        1,
        "raising an interrupt must not re-dispatch the action"
    );

    process_ready_node(&store, &armed_handler(&store).expect("armed"))
        .await
        .expect("handler");

    assert_eq!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("end"),
        "`resume next` settles the failed node Succeeded and takes its success edge"
    );
}

/// `retry` runs the handler in the gap between a failed attempt and the next dispatch, so it can fix
/// whatever the attempt needs first.
#[tokio::test]
async fn a_queued_retry_raises_the_retry_interrupt_before_re_dispatching() {
    let store = FakeStore::new();
    let hold = action_node(serde_json::json!({
        "retry": { "max_attempts": 5, "retry_on": "failure" }
    }));
    store.insert_workflow(workflow_with(
        flow_around(hold, "resume"),
        handler_for("retry"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("dispatch");
    let dispatched = store.latest_node_run("hold").expect("node run");
    store.resolve_node_run(dispatched.id, WorkflowStatus::Failed, None);

    // the failure lands and the retry policy queues another attempt, parking the run until the
    // backoff elapses.
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive");
    let queued = store.latest_node_run("hold").expect("node run");
    assert_eq!(queued.status, WorkflowStatus::Queued);
    assert_eq!(queued.transition_reason.as_deref(), Some("retry_queued"));

    // the backoff elapses and the retry row drives the node again — the interrupt fires in that gap.
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("the retry drive raises the interrupt");

    assert!(
        raised(&store),
        "a queued retry raises before the re-dispatch"
    );
    assert_eq!(
        store.dispatches().len(),
        1,
        "the second attempt must not be dispatched while the handler is deciding"
    );
    assert_eq!(
        store.latest_node_run("hold").expect("node run").status,
        WorkflowStatus::Queued
    );
}

/// `resolved` covers the parks an endpoint resolves out of band — a signal delivered, an approval
/// decided, an input submitted.
#[tokio::test]
async fn an_out_of_band_park_resolution_raises_resolved() {
    let store = FakeStore::new();
    let hold = serde_json::json!({
        "id": "hold", "kind": "signal",
        "parameters": { "name": "later" },
        "transitions": { "on_success": { "$node": "end" } }
    });
    store.insert_workflow(workflow_with(
        flow_around(hold, "resume"),
        handler_for("resolved"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    let parked = store.latest_node_run("hold").expect("node run");
    // exactly what the delivery endpoint does: stamp the parked run and wake the runtime.
    store.resolve_node_run(
        parked.id,
        WorkflowStatus::Succeeded,
        Some(runinator_models::json!({ "signal": "later", "payload": { "ok": true } })),
    );

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("the delivery raises the interrupt");
    assert!(raised(&store));
    assert_eq!(
        store.latest_node_run("hold").expect("node run").status,
        WorkflowStatus::Succeeded,
        "the resolution stands; the handler decides only what the thread does next"
    );

    process_ready_node(&store, &armed_handler(&store).expect("armed"))
        .await
        .expect("handler");
    assert_eq!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("end"),
        "control returns and the resolved signal follows its own success edge"
    );
}

/// `resolved` means *a park someone else resolved*, not "a node succeeded". an action's worker
/// result lands in exactly the same shape — a terminal node run found by a later drive with the
/// cursor still on the node — so without the kind filter every successful action in the workflow
/// would raise this.
///
/// (a polled `gate` needs no such guard: it opens and transitions inside one drive, so it never
/// leaves a resolved run for a later drive to find.)
#[tokio::test]
async fn a_successful_action_is_not_an_out_of_band_park_resolution() {
    let store = FakeStore::new();
    store.insert_workflow(workflow_with(
        flow_around(action_node(serde_json::json!({})), "resume"),
        handler_for("resolved"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("dispatch");
    let dispatched = store.latest_node_run("hold").expect("node run");
    store.resolve_node_run(dispatched.id, WorkflowStatus::Succeeded, None);

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("the result lands");

    assert!(
        !raised(&store),
        "an action finishing is the ordinary path, not something a park handler should see"
    );
    assert_eq!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("end"),
        "and the action follows its own success edge untouched"
    );
}

/// `external` has no node state to match: the ask is recorded on the run and the next drive of the
/// target thread raises it.
#[tokio::test]
async fn a_requested_interrupt_is_raised_by_the_next_drive_and_consumed() {
    let store = FakeStore::new();
    let hold = serde_json::json!({
        "id": "hold", "kind": "signal",
        "parameters": { "name": "later" },
        "transitions": { "on_success": { "$node": "end" } }
    });
    store.insert_workflow(workflow_with(
        flow_around(hold, "resume"),
        handler_for("external"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    assert!(!raised(&store), "nothing has been asked for yet");

    request_interrupt(&store, InterruptSource::External, None).await;
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("the request is picked up by the next drive");

    assert!(raised(&store));
    assert!(
        state(&store).pending_interrupts.is_empty(),
        "the request is consumed by the drive that raises it, or a retried write raises it twice"
    );

    let refresh = store.latest_node_run("refresh");
    assert!(refresh.is_none(), "the region has not been driven yet");
    process_ready_node(&store, &armed_handler(&store).expect("armed"))
        .await
        .expect("handler");
    assert!(store.latest_node_run("refresh").is_some());
}

/// a request nobody can service is dropped by the drive that looks at it. leaving it parked would
/// let it fire at an arbitrary later point in the run, long after the caller stopped expecting it.
#[tokio::test]
async fn a_requested_interrupt_with_no_handler_is_dropped_not_left_pending() {
    let store = FakeStore::new();
    let hold = serde_json::json!({
        "id": "hold", "kind": "signal",
        "parameters": { "name": "later" },
        "transitions": { "on_success": { "$node": "end" } }
    });
    // a `wake` handler is declared, so the feature is on — but nothing answers `external`.
    store.insert_workflow(workflow_with(flow_around(hold, "resume"), wake_handler()));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    request_interrupt(&store, InterruptSource::External, None).await;
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive");

    assert!(!raised(&store));
    assert!(
        state(&store).pending_interrupts.is_empty(),
        "an unserviceable request must not linger and fire later"
    );
}

/// a request naming another thread is not stolen by the one that happens to drive first.
#[tokio::test]
async fn a_targeted_request_is_left_alone_by_other_threads() {
    let store = FakeStore::new();
    let hold = serde_json::json!({
        "id": "hold", "kind": "signal",
        "parameters": { "name": "later" },
        "transitions": { "on_success": { "$node": "end" } }
    });
    store.insert_workflow(workflow_with(
        flow_around(hold, "resume"),
        handler_for("external"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("park");
    request_interrupt(&store, InterruptSource::External, Some(Uuid::now_v7())).await;
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive");

    assert!(!raised(&store));
    assert_eq!(
        state(&store).pending_interrupts.len(),
        1,
        "the request still belongs to the thread it named"
    );
}

/// `child` fires when the run a `subflow` node is parked on reaches a terminal, so a handler can
/// look at the outcome before the parent's own edges decide anything about it.
#[tokio::test]
async fn a_terminal_child_run_raises_the_child_interrupt() {
    let store = FakeStore::new();
    let child = serde_json::from_value::<WorkflowDefinition>(serde_json::json!({
        "id": "33333333-3333-3333-3333-333333333333",
        "name": "child",
        "version": "1.0.0",
        "enabled": true,
        "definition": {
            "start": "start",
            "nodes": [{ "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
                      { "id": "done", "kind": "end" }],
        }
    }))
    .expect("child workflow");
    store.insert_workflow(child);

    let hold = serde_json::json!({
        "id": "hold", "kind": "subflow",
        "subflow": { "workflow_name": "child", "subflow_type": "wait" },
        "transitions": {
            "on_success": { "$node": "end" },
            "on_failure": { "$node": "gave_up" }
        }
    });
    store.insert_workflow(workflow_with(
        flow_around(hold, "resume"),
        handler_for("child"),
    ));
    store.insert_run(queued_run());

    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("spawn the child and park");
    assert_eq!(
        store.latest_node_run("hold").expect("node run").status,
        WorkflowStatus::Waiting
    );
    let spawned = store
        .runs()
        .into_iter()
        .find(|run| run.id.to_string() != RUN_ID)
        .expect("the subflow spawned a child run");

    // still running: the parent is parked and nothing raises.
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("drive while the child is in flight");
    assert!(!raised(&store), "a child still running is not an event");

    store.settle_run(spawned.id, WorkflowStatus::Failed);
    process_ready_node(&store, &ready_node("hold"))
        .await
        .expect("the terminal child raises the interrupt");

    assert!(raised(&store));
    assert_eq!(
        store.latest_node_run("hold").expect("node run").status,
        WorkflowStatus::Waiting,
        "the parent must not settle while the handler is deciding about the child"
    );

    process_ready_node(&store, &armed_handler(&store).expect("armed"))
        .await
        .expect("handler");
    assert_eq!(
        store
            .run(RUN_ID.parse().unwrap())
            .expect("run")
            .active_node_id
            .as_deref(),
        Some("gave_up"),
        "control returns and the subflow node reads the failed child exactly as it would have"
    );
}

/// record an interrupt request the way the web service's endpoint does.
async fn request_interrupt(store: &FakeStore, source: InterruptSource, cursor_id: Option<Uuid>) {
    super::run_state::mutate_run_state(store, RUN_ID.parse().unwrap(), move |state| {
        state
            .pending_interrupts
            .push(PendingInterrupt::new(source, Value::Null, cursor_id));
    })
    .await
    .expect("record the request");
}
