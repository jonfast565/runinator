use super::context::{coerce_scalar_string, is_reentry_stale, runtime_context};
use super::transitions::{
    arm_node_timeout, time_out, timed_out_since_created, transition_from_node,
};
use super::*;
use uuid::Uuid;

struct AwaitParams {
    workflow_name: Option<String>,
    workflow_id: Option<Uuid>,
    run_id_expr: Option<Value>,
    task_run_id_expr: Option<Value>,
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
    let run_id_expr = params
        .get("run_id")
        .cloned()
        .filter(|value| !value.is_null());
    let task_run_id_expr = params
        .get("task_run_id")
        .cloned()
        .filter(|value| !value.is_null());
    AwaitParams {
        workflow_name,
        workflow_id,
        run_id_expr,
        task_run_id_expr,
        key_expr,
        mode: parse_await_mode(&params),
    }
}

/// resolve the await target to `(workflow id, name)` from an explicit id or a workflow name.
async fn resolve_target<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    params: &AwaitParams,
) -> Result<(Uuid, String), SendableError> {
    if let Some(id) = params.workflow_id {
        let name = ctx
            .db
            .fetch_workflow(id)
            .await?
            .map(|workflow| workflow.name)
            .ok_or_else(|| crate::errors::AWAIT_WORKFLOW_UNKNOWN.error(id))?;
        return Ok((id, name));
    }
    let Some(name) = params.workflow_name.clone() else {
        return Err(crate::errors::AWAIT_WORKFLOW_MISSING.error(&ctx.node.id));
    };
    let workflow = ctx
        .db
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
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    key_expr: Option<&Value>,
) -> Option<String> {
    let expr = key_expr?;
    let context = runtime_context(ctx).await;
    let resolved = runinator_workflows::resolve_value_refs(expr, &context).ok()?;
    coerce_scalar_string(&resolved)
}

async fn resolve_run_id<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    expression: Option<&Value>,
) -> Result<Option<Uuid>, SendableError> {
    let Some(expression) = expression else {
        return Ok(None);
    };
    let context = runtime_context(ctx).await;
    let resolved = runinator_workflows::resolve_value_refs(expression, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let raw = coerce_scalar_string(&resolved)
        .ok_or_else(|| "await task run_id must resolve to a UUID".to_string())?;
    raw.parse::<Uuid>()
        .map(Some)
        .map_err(|_| format!("await task run_id '{raw}' is not a UUID").into())
}

async fn resolve_task_run_id<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    expression: Option<&Value>,
) -> Result<Option<Uuid>, SendableError> {
    let Some(expression) = expression else {
        return Ok(None);
    };
    let context = runtime_context(ctx).await;
    let resolved = runinator_workflows::resolve_value_refs(expression, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let raw = coerce_scalar_string(&resolved)
        .ok_or_else(|| "await task task_run_id must resolve to a UUID".to_string())?;
    raw.parse::<Uuid>()
        .map(Some)
        .map_err(|_| format!("await task task_run_id '{raw}' is not a UUID").into())
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
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    target_id: Uuid,
    correlation: Option<&str>,
    exact_run_id: Option<Uuid>,
    since_unix: Option<i64>,
    mode: &str,
) -> Result<MatchSet, SendableError> {
    let runs = ctx.db.fetch_workflow_runs_for_workflow(target_id).await?;
    let mut matched_run_ids = Vec::new();
    let mut statuses = Vec::new();
    let mut any_match = false;
    let mut all_terminal = true;
    for run in runs {
        if run.id == ctx.workflow_run.id {
            continue;
        }
        if let Some(exact) = exact_run_id
            && run.id != exact
        {
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
    ctx: &super::handler::NodeHandlerContext<'_, T>,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        ctx.workflow_run.id,
        Some(ctx.node.id.clone()),
        "await_workflow_recheck",
        runinator_models::json!({ "node_id": ctx.node.id }),
    );
    ctx.db
        .enqueue_ready_node(event, ctx.node.id.clone(), Utc::now())
        .await?;
    Ok(())
}

/// process an await node: park the run until run(s) of a named workflow (optionally matching a
/// correlation key) reach a terminal state. resumption is event-driven — a matching terminal run
/// wakes this node via `maybe_wake_awaiters`; the optional node timeout fails the wait.
pub(super) struct AwaitRunHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for AwaitRunHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let params = parse_await_params(ctx.node);
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));

        // parked re-entry: woken by a matching terminal run or the timeout re-arm.
        if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
            let Ok(state) = AwaitWorkflowState::from_wire_value(&node_run.state) else {
                return Ok(ReadyNodeDisposition::Complete);
            };
            if timed_out_since_created(ctx.timing(), node_run) {
                time_out(ctx, node_run, "Await workflow timed out").await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            if let Some(task_run_id) = state.exact_task_run_id {
                let task = ctx
                    .db
                    .fetch_workflow_task_run(task_run_id)
                    .await?
                    .ok_or_else(|| format!("await task {task_run_id} does not exist"))?;
                if task.status.is_terminal() {
                    transition_task_await(ctx, node_run, task_run_id, &task).await?;
                    return Ok(ReadyNodeDisposition::Complete);
                }
                // Task results do not touch their already-completed launcher nodes. Until a
                // future push wake is available, use a durable short recheck to keep an exact
                // task await responsive across worker/result consumer restarts.
                enqueue_await_recheck(ctx, Utc::now() + chrono::Duration::seconds(1)).await?;
                return Ok(ReadyNodeDisposition::KeepClaim);
            }
            let matches = evaluate_matches(
                ctx,
                state.workflow_id,
                state.correlation_value.as_deref(),
                state.exact_run_id,
                state.since_unix,
                &state.mode,
            )
            .await?;
            if matches.satisfied {
                transition_await(ctx, node_run, state.workflow_id, &state.mode, matches).await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            return Ok(ReadyNodeDisposition::KeepClaim);
        }

        // first visit: resolve the target + correlation, check immediately, else park.
        let exact_run_id = resolve_run_id(ctx, params.run_id_expr.as_ref()).await?;
        let exact_task_run_id = resolve_task_run_id(ctx, params.task_run_id_expr.as_ref()).await?;
        if let Some(task_run_id) = exact_task_run_id {
            let task = ctx
                .db
                .fetch_workflow_task_run(task_run_id)
                .await?
                .ok_or_else(|| format!("await task {task_run_id} does not exist"))?;
            let node_run = ctx
                .db
                .create_workflow_node_run(
                    ctx.workflow_run.id,
                    ctx.node.id.clone(),
                    ctx.node.parameters.clone().into(),
                    super::context::most_recently_finished_node_run(ctx.node_runs),
                    Some(ctx.cursor),
                )
                .await?;
            if task.status.is_terminal() {
                transition_task_await(ctx, &node_run, task_run_id, &task).await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            let state = AwaitWorkflowState {
                workflow_id: ctx.workflow_run.workflow_id,
                workflow_name: "task".into(),
                exact_run_id: None,
                exact_task_run_id: Some(task_run_id),
                correlation_value: None,
                since_unix: None,
                mode: "all".into(),
                deadline_unix: ctx.node.timeout_seconds.map(|t| Utc::now().timestamp() + t),
            };
            ctx.db
                .update_workflow_node_run(
                    node_run.id,
                    WorkflowStatus::Waiting,
                    Some(node_run.attempt + 1),
                    None,
                    None,
                    Some(state.to_wire_value()?),
                    Some("await_task_waiting".into()),
                    None,
                )
                .await?;
            ctx.db
                .update_workflow_run_status(
                    ctx.workflow_run.id,
                    WorkflowStatus::Waiting,
                    Some(ctx.node.id.clone()),
                    None,
                    None,
                )
                .await?;
            arm_node_timeout(ctx).await?;
            enqueue_await_recheck(ctx, Utc::now() + chrono::Duration::seconds(1)).await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        let (target_id, target_name) = if let Some(run_id) = exact_run_id {
            let run = ctx
                .db
                .fetch_workflow_run(run_id)
                .await?
                .ok_or_else(|| format!("await task run {run_id} does not exist"))?;
            let workflow = ctx
                .db
                .fetch_workflow(run.workflow_id)
                .await?
                .ok_or_else(|| crate::errors::AWAIT_WORKFLOW_UNKNOWN.error(run.workflow_id))?;
            (run.workflow_id, workflow.name)
        } else {
            resolve_target(ctx, &params).await?
        };
        let correlation = resolve_key(ctx, params.key_expr.as_ref()).await;
        let since_unix = Some(
            ctx.workflow_run
                .started_at
                .unwrap_or(ctx.workflow_run.created_at)
                .timestamp(),
        );
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let matches = evaluate_matches(
            ctx,
            target_id,
            correlation.as_deref(),
            exact_run_id,
            since_unix,
            &params.mode,
        )
        .await?;
        if matches.satisfied {
            transition_await(ctx, &node_run, target_id, &params.mode, matches).await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        let state = AwaitWorkflowState {
            workflow_id: target_id,
            workflow_name: target_name,
            correlation_value: correlation.clone(),
            exact_run_id,
            exact_task_run_id: None,
            since_unix,
            mode: params.mode.clone(),
            deadline_unix: ctx.node.timeout_seconds.map(|t| Utc::now().timestamp() + t),
        };
        ctx.db
            .update_workflow_node_run(
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
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::Waiting,
                Some(ctx.node.id.clone()),
                None,
                None,
            )
            .await?;
        arm_node_timeout(ctx).await?;
        // re-check after committing the park: a matching run that reached terminal during the first-visit
        // window would otherwise be missed, since the wake path only finds this node once it is `Waiting`.
        let recheck = evaluate_matches(
            ctx,
            target_id,
            correlation.as_deref(),
            exact_run_id,
            since_unix,
            &params.mode,
        )
        .await?;
        if recheck.satisfied {
            enqueue_await_wake(ctx).await?;
        }
        Ok(ReadyNodeDisposition::Complete)
    }
}

async fn enqueue_await_recheck<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    ready_at: chrono::DateTime<Utc>,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        ctx.workflow_run.id,
        Some(ctx.node.id.clone()),
        "await_task_recheck",
        runinator_models::json!({ "node_id": ctx.node.id }),
    );
    ctx.db
        .enqueue_ready_node(event, ctx.node.id.clone(), ready_at)
        .await?;
    Ok(())
}

async fn transition_task_await<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    node_run: &WorkflowNodeRun,
    task_run_id: Uuid,
    task: &WorkflowTaskRun,
) -> Result<(), SendableError> {
    transition_from_node(
        ctx,
        node_run,
        task.status,
        Some(runinator_models::json!({
            "task_run_id": task_run_id,
            "status": task.status.as_str(),
            "output": task.output_json.clone(),
        })),
        task.message.clone(),
    )
    .await
    .map(|_| ())
}

async fn transition_await<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    node_run: &WorkflowNodeRun,
    workflow_id: Uuid,
    mode: &str,
    matches: MatchSet,
) -> Result<(), SendableError> {
    let output = AwaitWorkflowOutput {
        workflow_id,
        matched_run_ids: matches.matched_run_ids,
        mode: mode.to_string(),
        statuses: matches.statuses,
    };
    transition_from_node(
        ctx,
        node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some("await_workflow_satisfied".into()),
    )
    .await?;
    Ok(())
}
