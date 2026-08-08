// the debugger's execution half: the break decision, the inspection snapshot, and the shadowing
// that keeps a speculative cursor from escaping the run.
//
// the debug frame has always been persisted — breakpoints, step requests, the mode — but nothing in
// the reducer ever read it, so a "paused" run kept running. this module is the reader.

use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;
use runinator_models::debug::{
    DEBUG_SHADOW_REPLAY, DEBUG_SHADOW_STUB, DEBUG_SPECULATIVE, Debuggable,
};
use runinator_models::workflow_state::DebugRuntime;

/// what the debugger says should happen to the node a cursor is about to execute.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DebugGate {
    /// run the node normally.
    Proceed,
    /// stop before it; only a debug command revives this cursor.
    Park,
    /// record a synthetic outcome instead of letting the node reach the outside world.
    Shadow,
}

/// node kinds whose processing escapes the run — a worker dispatch, an emitted artifact, an audit
/// row, a child run, a human prompt.
///
/// a speculative cursor shadows these unless the operator armed the specific node. deciding it here
/// rather than inside each handler is what makes it impossible for a new node kind to quietly
/// acquire the ability to escape a "what if" branch.
pub(super) fn has_external_effect(kind: &WorkflowNodeKind) -> bool {
    matches!(
        kind,
        WorkflowNodeKind::Action
            | WorkflowNodeKind::Output
            | WorkflowNodeKind::Signal
            | WorkflowNodeKind::Audit
            | WorkflowNodeKind::Checkpoint
            | WorkflowNodeKind::Subflow
            | WorkflowNodeKind::Map
            | WorkflowNodeKind::Approval
            | WorkflowNodeKind::Input
    )
}

/// decide whether this cursor may execute `node`.
///
/// ordering matters: a step request is consumed on entry, which is exactly what makes "step" advance
/// one node — the next node re-evaluates and breaks again. the expensive context snapshot is only
/// built once the break decision is already true, so a non-breaking node costs one state parse.
pub(super) async fn debug_gate<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<DebugGate, SendableError> {
    let state = WorkflowRunState::from_state(&workflow_run.state);
    let Some(frame) = state.debug.as_ref() else {
        return Ok(speculative_gate(cursor, node));
    };
    if !frame.config.enabled {
        return Ok(speculative_gate(cursor, node));
    }
    let runtime = state.cursor_debug(cursor.id);

    // a step was requested for this thread of control: spend it and run exactly this node.
    if runtime.step_requested {
        let cleared = DebugRuntime {
            paused: false,
            step_requested: false,
            one_shot_breakpoint: runtime
                .one_shot_breakpoint
                .clone()
                .filter(|target| target != node.id.as_str()),
            ..runtime.clone()
        };
        persist_runtime(db, workflow_run, cursor, cleared, None).await?;
        return Ok(speculative_gate(cursor, node));
    }
    // already parked here. re-parking is idempotent, so a duplicated drive can never slip the node
    // past a breakpoint.
    if runtime.paused {
        return Ok(DebugGate::Park);
    }
    if !workflow_run.should_break_at(cursor) {
        return Ok(speculative_gate(cursor, node));
    }

    let context = runtime_context(db, workflow_run, cursor, node_runs).await;
    let snapshot = DebugRuntime {
        paused: true,
        step_requested: false,
        one_shot_breakpoint: runtime
            .one_shot_breakpoint
            .clone()
            .filter(|target| target != node.id.as_str()),
        current_node_id: Some(node.id.clone()),
        current_node_kind: Some(node.kind.clone()),
        input_json: Some(input_snapshot(node, &context)),
        context_json: Some(context),
        last_output_json: cursor.last_output.clone(),
    };
    persist_runtime(
        db,
        workflow_run,
        cursor,
        snapshot,
        Some(format!("Paused before {}", node.id)),
    )
    .await?;
    Ok(DebugGate::Park)
}

/// a speculative cursor may only reach the outside world through a node it was explicitly armed for.
fn speculative_gate(cursor: &RunCursor, node: &WorkflowNode) -> DebugGate {
    if cursor.is_speculative() && has_external_effect(&node.kind) && !cursor.is_armed_for(&node.id)
    {
        return DebugGate::Shadow;
    }
    DebugGate::Proceed
}

/// the node's parameters as the operator would read them, resolved against the live context.
///
/// deliberately not the action path's parameter builder: that one has `std.exec`/`std.code` special
/// cases and does database reads, and a snapshot shown to a human must never be able to fail the
/// run. an unresolvable ref falls back to the authored value.
fn input_snapshot(node: &WorkflowNode, context: &Value) -> Value {
    let parameters: Value = node.parameters.clone().into();
    runinator_workflows::resolve_value_refs(&parameters, context).unwrap_or(parameters)
}

/// write one cursor's debugger runtime, taking `DebugPaused` only when no cursor can still advance.
async fn persist_runtime<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    runtime: DebugRuntime,
    message: Option<String>,
) -> Result<(), SendableError> {
    run_state::park_cursor_for_debug(db, workflow_run.id, cursor.id, runtime, message).await
}

/// settle an externally-visible node for a speculative cursor without letting it act.
///
/// resolution order: replay what the real run already recorded for this node, then what the run this
/// one was replayed from recorded, then a stub. replaying is what makes a "what if" fork meaningful
/// — the branch walks with the real values everywhere except where the operator patched them.
pub(super) async fn shadow_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let (status, output, reason) = match replay_source(db, workflow_run, node, node_runs).await? {
        Some((status, output)) => (status, output, DEBUG_SHADOW_REPLAY),
        None => (
            WorkflowStatus::Succeeded,
            runinator_models::json!({
                "shadow": true,
                "node_id": node.id,
                "kind": node.kind,
            }),
            DEBUG_SHADOW_STUB,
        ),
    };
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
            Some(cursor),
        )
        .await?;
    tracing::debug!(
        node_id = %node.id,
        cursor_id = %cursor.id,
        reason,
        "shadowing an external-effect node for a speculative cursor"
    );
    transition_from_node(
        db,
        workflow_run,
        cursor,
        node,
        &node_run,
        status,
        Some(output),
        Some(format!("{DEBUG_SPECULATIVE}:{reason}")),
        node_runs,
    )
    .await?;
    Ok(())
}

/// the recorded outcome this node had on a real thread of control, if there is one to replay.
async fn replay_source<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<Option<(WorkflowStatus, Value)>, SendableError> {
    // the real run's own record wins: it is this run, this graph, these inputs.
    if let Some(recorded) = node_runs
        .iter()
        .filter(|run| run.node_id == node.id && !run.speculative && run.status.is_terminal())
        .max_by_key(|run| run.id)
    {
        return Ok(Some((
            recorded.status,
            recorded.output_json.clone().unwrap_or(Value::Null),
        )));
    }
    // otherwise fall back to the run this one was replayed from, which `replay_workflow_run`
    // stamps into state when it clones a run for debugging.
    let source_run_id = WorkflowRunState::from_state(&workflow_run.state)
        .extra
        .get("replay")
        .and_then(|replay| replay.get("source_run_id"))
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok());
    let Some(source_run_id) = source_run_id else {
        return Ok(None);
    };
    let source_runs = db.fetch_workflow_node_runs(source_run_id).await?;
    Ok(source_runs
        .iter()
        .filter(|run| run.node_id == node.id && run.status.is_terminal())
        .max_by_key(|run| run.id)
        .map(|run| (run.status, run.output_json.clone().unwrap_or(Value::Null))))
}
