use super::context::{coerce_scalar_string, is_reentry_stale, runtime_context};
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;
use uuid::Uuid;

struct AwaitParams {
    workflow_name: Option<String>,
    workflow_id: Option<Uuid>,
    key_expr: Option<Value>,
    mode: String,
}

/// parse the await mode ("all"|"any"), defaulting to "all".
pub(super) fn parse_await_mode(params: &Value) -> String {
    params
        .get("mode")
        .and_then(Value::as_str)
        .filter(|m| matches!(*m, "all" | "any"))
        .unwrap_or("all")
        .to_string()
}

fn parse_await_params(node: &WorkflowNode) -> AwaitParams {
    let params: Value = node.parameters.clone().into();
    let workflow_name = params
        .get("workflow")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let workflow_id = params
        .get("workflow_id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok());
    let key_expr = params.get("key").cloned().filter(|value| !value.is_null());
    AwaitParams {
        workflow_name,
        workflow_id,
        key_expr,
        mode: parse_await_mode(&params),
    }
}

/// resolve the await target to `(workflow id, name)` from an explicit id or a workflow name.
async fn resolve_target<T: ReducerStore>(
    db: &T,
    node: &WorkflowNode,
    params: &AwaitParams,
) -> Result<(Uuid, String), SendableError> {
    if let Some(id) = params.workflow_id {
        let name = db
            .fetch_workflow(id)
            .await?
            .map(|workflow| workflow.name)
            .ok_or_else(|| crate::errors::AWAIT_WORKFLOW_UNKNOWN.error(id))?;
        return Ok((id, name));
    }
    let Some(name) = params.workflow_name.clone() else {
        return Err(crate::errors::AWAIT_WORKFLOW_MISSING.error(&node.id));
    };
    let workflow = db
        .fetch_workflow_by_name(name.clone())
        .await?
        .ok_or_else(|| crate::errors::AWAIT_WORKFLOW_UNKNOWN.error(&name))?;
    let id = workflow
        .id
        .ok_or_else(|| crate::errors::AWAIT_WORKFLOW_UNKNOWN.error(&name))?;
    Ok((id, name))
}

/// resolve the optional correlation-key expression against the run context into a flat string.
async fn resolve_key<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node_runs: &[WorkflowNodeRun],
    key_expr: Option<&Value>,
) -> Option<String> {
    let expr = key_expr?;
    let context = runtime_context(db, workflow_run, cursor, node_runs).await;
    let resolved = runinator_workflows::resolve_value_refs(expr, &context).ok()?;
    coerce_scalar_string(&resolved)
}

struct MatchSet {
    matched_run_ids: Vec<Uuid>,
    statuses: Vec<String>,
    satisfied: bool,
}

/// decide whether the await policy is met. an empty match set is never satisfied (so `all` does not
/// vacuously succeed); `any` needs at least one terminal match, `all` needs a match and all terminal.
pub(super) fn await_satisfied(
    mode: &str,
    any_match: bool,
    has_terminal: bool,
    all_terminal: bool,
) -> bool {
    match mode {
        "any" => has_terminal,
        _ => any_match && all_terminal,
    }
}

/// scan runs of the target workflow and decide whether the await policy is met. only runs started at
/// or after `since_unix` (and, when a correlation is set, carrying that key) count; the awaiter's own
/// run is excluded. an empty match set is never satisfied, so `all` does not vacuously succeed.
async fn evaluate_matches<T: ReducerStore>(
    db: &T,
    self_run_id: Uuid,
    target_id: Uuid,
    correlation: Option<&str>,
    since_unix: Option<i64>,
    mode: &str,
) -> Result<MatchSet, SendableError> {
    let runs = db.fetch_workflow_runs_for_workflow(target_id).await?;
    let mut matched_run_ids = Vec::new();
    let mut statuses = Vec::new();
    let mut any_match = false;
    let mut all_terminal = true;
    for run in runs {
        if run.id == self_run_id {
            continue;
        }
        if let Some(since) = since_unix
            && run.created_at.timestamp() < since
        {
            continue;
        }
        if let Some(key) = correlation
            && run.correlation_key.as_deref() != Some(key)
        {
            continue;
        }
        any_match = true;
        if run.status.is_terminal() {
            matched_run_ids.push(run.id);
            statuses.push(run.status.as_str().to_string());
        } else {
            all_terminal = false;
        }
    }
    let satisfied = await_satisfied(mode, any_match, !matched_run_ids.is_empty(), all_terminal);
    Ok(MatchSet {
        matched_run_ids,
        statuses,
        satisfied,
    })
}

/// enqueue an immediate self ready-node so the parked await re-drives at once. used to close the
/// check-then-park race when a matching run reached terminal while this node was parking.
async fn enqueue_await_wake<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    node: &WorkflowNode,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "await_workflow_recheck",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), Utc::now())
        .await?;
    Ok(())
}

/// process an await node: park the run until run(s) of a named workflow (optionally matching a
/// correlation key) reach a terminal state. resumption is event-driven — a matching terminal run
/// wakes this node via `maybe_wake_awaiters`; the optional node timeout fails the wait.
pub(super) async fn process_await_run_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    let params = parse_await_params(node);
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs, cursor));

    // parked re-entry: woken by a matching terminal run or the timeout re-arm.
    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        let Ok(state) = AwaitWorkflowState::from_wire_value(&node_run.state) else {
            return Ok(ReadyNodeDisposition::Complete);
        };
        if timed_out_since_created(node, node_run) {
            time_out(
                db,
                workflow_run,
                cursor,
                node,
                node_run,
                "Await workflow timed out",
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        let matches = evaluate_matches(
            db,
            workflow_run.id,
            state.workflow_id,
            state.correlation_value.as_deref(),
            state.since_unix,
            &state.mode,
        )
        .await?;
        if matches.satisfied {
            transition_await(
                db,
                workflow_run,
                cursor,
                node,
                node_run,
                state.workflow_id,
                &state.mode,
                matches,
                node_runs,
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        return Ok(ReadyNodeDisposition::KeepClaim);
    }

    // first visit: resolve the target + correlation, check immediately, else park.
    let (target_id, target_name) = resolve_target(db, node, &params).await?;
    let correlation = resolve_key(
        db,
        workflow_run,
        cursor,
        node_runs,
        params.key_expr.as_ref(),
    )
    .await;
    let since_unix = Some(
        workflow_run
            .started_at
            .unwrap_or(workflow_run.created_at)
            .timestamp(),
    );
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
            Some(cursor),
        )
        .await?;
    let matches = evaluate_matches(
        db,
        workflow_run.id,
        target_id,
        correlation.as_deref(),
        since_unix,
        &params.mode,
    )
    .await?;
    if matches.satisfied {
        transition_await(
            db,
            workflow_run,
            cursor,
            node,
            &node_run,
            target_id,
            &params.mode,
            matches,
            node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }
    let state = AwaitWorkflowState {
        workflow_id: target_id,
        workflow_name: target_name,
        correlation_value: correlation.clone(),
        since_unix,
        mode: params.mode.clone(),
        deadline_unix: node.timeout_seconds.map(|t| Utc::now().timestamp() + t),
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.to_wire_value()?),
        Some("await_workflow_waiting".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    arm_node_timeout(db, workflow_run.id, cursor, node).await?;
    // re-check after committing the park: a matching run that reached terminal during the first-visit
    // window would otherwise be missed, since the wake path only finds this node once it is `Waiting`.
    let recheck = evaluate_matches(
        db,
        workflow_run.id,
        target_id,
        correlation.as_deref(),
        since_unix,
        &params.mode,
    )
    .await?;
    if recheck.satisfied {
        enqueue_await_wake(db, workflow_run.id, node).await?;
    }
    Ok(ReadyNodeDisposition::Complete)
}

#[allow(clippy::too_many_arguments)]
async fn transition_await<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    workflow_id: Uuid,
    mode: &str,
    matches: MatchSet,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let output = AwaitWorkflowOutput {
        workflow_id,
        matched_run_ids: matches.matched_run_ids,
        mode: mode.to_string(),
        statuses: matches.statuses,
    };
    transition_from_node(
        db,
        workflow_run,
        cursor,
        node,
        node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some("await_workflow_satisfied".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) struct AwaitRunHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for AwaitRunHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_await_run_node(
                ctx.db,
                ctx.workflow_run,
                ctx.cursor,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await
        }
    }
}
