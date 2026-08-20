use super::context::{coerce_scalar_string, runtime_context, set_step_output};
use super::execution::{NodeStepContext, NodeTimingContext, WorkflowRunContext};
use super::*;
use chrono::DateTime;
use runinator_models::workflows::WorkflowRetry;
use uuid::Uuid;

// --- shared store-backed runtime helpers -----------------------------------------

/// settle a node run, retrying while attempts remain, otherwise transitioning.
pub(super) async fn retry_or_transition<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
) -> Result<(), SendableError> {
    transition_from_node(ctx, node_run, status, output_json, message).await?;
    Ok(())
}

/// exponential backoff from the node's retry config: `base * 2^(attempt-1)`, capped at `max`, with
/// optional jitter spreading the delay into `[delay/2, delay]` so simultaneous retries disperse.
fn retry_backoff_delay(retry: &WorkflowRetry, attempt: i64) -> chrono::Duration {
    let base = retry.backoff_base_seconds.max(0);
    let cap = retry.backoff_max_seconds.max(base);
    let exponent = attempt.saturating_sub(1).clamp(0, 30) as u32;
    let mut seconds = base
        .saturating_mul(2_i64.saturating_pow(exponent))
        .clamp(base, cap);
    if retry.jitter && seconds > 1 {
        // cheap, dependency-free jitter: fold sub-second clock noise into the lower half.
        let span = seconds / 2;
        let noise = (Utc::now().timestamp_subsec_nanos() as i64) % (span + 1);
        seconds = (seconds - span) + noise;
    }
    chrono::Duration::seconds(seconds)
}

/// time out the in-flight run with a node-specific message, retrying if attempts remain.
pub(super) async fn time_out<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    node_run: &WorkflowNodeRun,
    message: &str,
) -> Result<(), SendableError> {
    retry_or_transition(
        ctx,
        node_run,
        WorkflowStatus::TimedOut,
        None,
        Some(message.into()),
    )
    .await
}

/// create a node run and block this thread of control with a message.
pub(super) async fn block_node<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    message: &str,
) -> Result<(), SendableError> {
    let node_run = ctx
        .db
        .create_workflow_node_run(
            ctx.workflow_run.id,
            ctx.node.id.clone(),
            ctx.node.parameters.clone().into(),
            None,
            Some(ctx.cursor),
        )
        .await?;
    ctx.db
        .update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Blocked,
            Some(node_run.attempt + 1),
            None,
            None,
            None,
            Some(WorkflowStatus::Blocked.as_str().into()),
            Some(message.into()),
        )
        .await?;
    // a blocked thread of control is *stuck*, not finished: it stays exactly where it is, keeping
    // its loop/try frames, so an operator can inspect it and a later drive can retry from the same
    // place. retiring it here would leave a live (non-terminal) run with no cursor to drive, and
    // silently discard the frames that say which iteration it was on.
    ctx.db
        .update_workflow_run_status(
            ctx.workflow_run.id,
            WorkflowStatus::Blocked,
            Some(ctx.node.id.clone()),
            None,
            Some(message.into()),
        )
        .await
}

/// advance a try node into a phase (body/catch/finally), recording the phase frame.
/// advance a try node into a phase (body/catch/finally), recording the phase frame on the cursor.
///
/// the frame belongs to this thread of control, not the run: two branches inside a try region would
/// otherwise share one phase and each would see the other's.
pub(super) async fn start_try_phase<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    node_run: &WorkflowNodeRun,
    target: &str,
    phase: &str,
    pending_status: Option<WorkflowStatus>,
    pending_output: Option<Value>,
) -> Result<(), SendableError> {
    let frame = TryFrame {
        node_id: ctx.node.id.clone(),
        phase: phase.into(),
        pending_status,
        pending_output,
    };
    ctx.db
        .update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Running,
            Some(node_run.attempt + 1),
            None,
            None,
            Some(frame.to_wire_value()?),
            Some(format!("try_{phase}_started")),
            None,
        )
        .await?;
    let staged = frame.clone();
    run_state::mutate_cursor(ctx.db, ctx.workflow_run.id, ctx.cursor.id, move |cursor| {
        cursor.try_frame = Some(staged.clone());
    })
    .await?;
    run_state::advance_cursor(
        ctx.db,
        ctx.workflow_run.id,
        ctx.cursor.id,
        WorkflowStatus::Running,
        run_state::CursorMove::To(target.to_string()),
        None,
    )
    .await
}

/// true when the run started more than `node.timeout_seconds` ago.
pub(super) fn timed_out(ctx: NodeTimingContext<'_>, run: &WorkflowNodeRun) -> bool {
    let Some(timeout) = ctx.node.timeout_seconds else {
        return false;
    };
    let Some(started) = run.started_at else {
        return false;
    };
    Utc::now() - started > chrono::Duration::seconds(timeout) + ctx.cursor.suspension_credit()
}

/// a park never goes `Running`, so its deadline runs from when the node run was created rather than
/// from `started_at`. every timeout here is extended by whatever time the thread spent frozen behind
/// an interrupt: a handler is not the thing the node is waiting for, so its duration must not be
/// charged to the wait.
pub(super) fn timed_out_since_created(ctx: NodeTimingContext<'_>, run: &WorkflowNodeRun) -> bool {
    let Some(timeout) = ctx.node.timeout_seconds else {
        return false;
    };
    Utc::now() - run.created_at
        > chrono::Duration::seconds(timeout) + ctx.cursor.suspension_credit()
}

/// like `timed_out_since_created`, but falls back to `default_timeout_seconds` when the node
/// declares no timeout — for parks that must not wait forever.
pub(super) fn timed_out_since_created_or(
    ctx: NodeTimingContext<'_>,
    run: &WorkflowNodeRun,
    default_timeout_seconds: i64,
) -> bool {
    let timeout = ctx.node.timeout_seconds.unwrap_or(default_timeout_seconds);
    Utc::now() - run.created_at
        > chrono::Duration::seconds(timeout) + ctx.cursor.suspension_credit()
}

/// enqueue a delayed self ready node at a node's timeout deadline. the event-driven ready queue does
/// not re-poll parked nodes, so a node that parks (approval/join/subflow) re-arms its own timeout so
/// the timeout check fires even when no external wake-up arrives.
pub(super) async fn arm_node_timeout<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
) -> Result<(), SendableError> {
    let Some(timeout) = ctx.node.timeout_seconds else {
        return Ok(());
    };
    arm_node_timeout_in(ctx, timeout).await
}

/// like `arm_node_timeout`, but always arms, falling back to `default_timeout_seconds` when the
/// node declares no timeout — for parks whose timeout check must fire even without one configured.
pub(super) async fn arm_node_timeout_or<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    default_timeout_seconds: i64,
) -> Result<(), SendableError> {
    let timeout = ctx.node.timeout_seconds.unwrap_or(default_timeout_seconds);
    arm_node_timeout_in(ctx, timeout).await
}

pub(super) async fn arm_node_timeout_in<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    timeout_seconds: i64,
) -> Result<(), SendableError> {
    let deadline = Utc::now() + chrono::Duration::seconds(timeout_seconds);
    arm_cursor_wake(
        ctx.db,
        ctx.workflow_run.id,
        ctx.cursor.id,
        &ctx.node.id,
        "node_timeout_rearm",
        runinator_models::json!({ "node_id": ctx.node.id }),
        deadline,
    )
    .await
}

/// when a child workflow run reaches a terminal state, wake the parent subflow node waiting on it.
/// the parent linkage is stamped into the child run's `state.subflow_parent` at creation.
pub(super) async fn maybe_wake_subflow_parent<T: RuntimeStore>(
    ctx: &WorkflowRunContext<'_, T>,
) -> Result<(), SendableError> {
    let run = ctx.workflow_run;
    if !run.status.is_terminal() {
        return Ok(());
    }
    let Some(parent) = run.execution_state.subflow_parent.as_ref() else {
        return Ok(());
    };
    let parent_run_id = parent.run_id;
    let parent_node_id = parent.node_id.as_str();
    let event = NewOrchestrationEvent::new(
        parent_run_id,
        Some(parent_node_id.to_string()),
        "subflow_child_finished",
        runinator_models::json!({ "child_run_id": run.id, "status": run.status.as_str() }),
    );
    ctx.db
        .enqueue_ready_node(event, parent_node_id.to_string(), Utc::now())
        .await?;
    Ok(())
}

/// when a run has no correlation key yet, resolve the workflow's `metadata.correlation` expression
/// against the live context and stamp it write-once. lets `await workflow ... key` joins match this
/// run by a value it derives from input or a mid-run step output.
async fn maybe_stamp_correlation<T: RuntimeStore>(
    ctx: &WorkflowRunContext<'_, T>,
    context: &Value,
) -> Result<(), SendableError> {
    if ctx.workflow_run.correlation_key.is_some() {
        return Ok(());
    }
    let Some(snapshot) = ctx.workflow_run.workflow_snapshot.as_ref() else {
        return Ok(());
    };
    let Some(expression) = snapshot.definition.metadata.get("correlation") else {
        return Ok(());
    };
    let Ok(resolved) = runinator_workflows::resolve_value_refs(expression, context) else {
        return Ok(());
    };
    if let Some(key) = coerce_scalar_string(&resolved) {
        ctx.db
            .set_run_correlation_key(ctx.workflow_run.id, key)
            .await?;
    }
    Ok(())
}

/// when any run reaches a terminal state, wake await-workflow nodes parked on a run of that workflow
/// (optionally matching a correlation value and start-time window). scans waiting node runs and
/// nudges each matching awaiter; the awaiter's handler re-checks satisfaction on wake.
pub(super) async fn maybe_wake_awaiters<T: RuntimeStore>(
    ctx: &WorkflowRunContext<'_, T>,
) -> Result<(), SendableError> {
    let run = ctx.workflow_run;
    if !run.status.is_terminal() {
        return Ok(());
    }
    let waiting = ctx
        .db
        .fetch_workflow_node_runs_by_status(WorkflowStatus::Waiting)
        .await?;
    for node_run in waiting {
        let Ok(state) = AwaitWorkflowState::from_wire_value(&node_run.state) else {
            continue;
        };
        if state.workflow_id != run.workflow_id {
            continue;
        }
        if let Some(exact) = state.exact_run_id
            && run.id != exact
        {
            continue;
        }
        if let Some(since) = state.since_unix
            && run.created_at.timestamp() < since
        {
            continue;
        }
        if let Some(expected) = state.correlation_value.as_deref()
            && run.correlation_key.as_deref() != Some(expected)
        {
            continue;
        }
        let event = NewOrchestrationEvent::new(
            node_run.workflow_run_id,
            Some(node_run.node_id.clone()),
            "await_workflow_finished",
            runinator_models::json!({ "finished_run_id": run.id, "status": run.status.as_str() }),
        );
        ctx.db
            .enqueue_ready_node(event, node_run.node_id.clone(), Utc::now())
            .await?;
    }
    Ok(())
}

/// settle a node and move the thread of control that ran it.
///
/// every write to the run's position goes through [`run_state::advance_cursor`], which applies the
/// move and the run status in one compare-and-swap. that is what lets several branches settle
/// concurrently without discarding each other's frames — and what encodes the rule that a run only
/// *succeeds* when its last cursor retires, while a failure ends it immediately.
pub(super) async fn transition_from_node<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
) -> Result<Option<String>, SendableError> {
    settle_node(ctx, node_run, status, output_json, message, true).await
}

/// like [`transition_from_node`], but `retry_eligible` decides whether the node's own retry policy
/// may intercept `status`.
///
/// an organic dispatch result is retry-eligible: the policy the author wrote down gets to decide
/// whether to try again. a status an interrupt handler chose via `resume continue`/`resume fail` is
/// not — it is an explicit decision, not a result the policy is entitled to second-guess, so it must
/// reach the node the way the handler said it should.
pub(super) async fn settle_node<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    retry_eligible: bool,
) -> Result<Option<String>, SendableError> {
    if retry_eligible
        && ctx.node.retry.retry_on.retryable(status)
        && node_run.attempt < ctx.node.retry.max_attempts
    {
        schedule_node_retry(ctx, node_run, output_json, message).await?;
        return Ok(Some(ctx.node.id.clone()));
    }

    ctx.db
        .update_workflow_node_run(
            node_run.id,
            status,
            None,
            None,
            output_json.clone(),
            None,
            Some(status.as_str().into()),
            message.clone(),
        )
        .await?;
    let mut context = runtime_context(ctx).await;
    if let Some(output) = output_json.clone() {
        set_step_output(&mut context, &ctx.node.id, output);
    }
    // the debugger's "last output" pane is per-branch; run-wide "most recently finished" is simply
    // the wrong answer under fan-out. only paid for by runs that are actually being debugged.
    if ctx.workflow_run.execution_state.debug.is_some() {
        let last = output_json.clone().unwrap_or(Value::Null);
        run_state::mutate_cursor(ctx.db, ctx.workflow_run.id, ctx.cursor.id, |cursor| {
            cursor.last_output = Some(last.clone());
        })
        .await?;
    }
    maybe_stamp_correlation(ctx, &context).await?;
    let next = runinator_workflows::next_transition(ctx.node, status, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    match next {
        Some(next) => {
            run_state::advance_cursor(
                ctx.db,
                ctx.workflow_run.id,
                ctx.cursor.id,
                WorkflowStatus::Running,
                run_state::CursorMove::To(next.clone()),
                message,
            )
            .await?;
            Ok(Some(next))
        }
        // no outgoing edge: this thread of control is done. the run takes the terminal only once
        // every other branch has also retired.
        None => {
            run_state::advance_cursor(
                ctx.db,
                ctx.workflow_run.id,
                ctx.cursor.id,
                status,
                run_state::CursorMove::Retire,
                message,
            )
            .await?;
            Ok(None)
        }
    }
}

async fn schedule_node_retry<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    node_run: &WorkflowNodeRun,
    output_json: Option<Value>,
    message: Option<String>,
) -> Result<(), SendableError> {
    let next_attempt = node_run.attempt + 1;
    let delay = retry_backoff_delay(&ctx.node.retry, node_run.attempt);
    let ready_at = Utc::now() + delay;
    ctx.db
        .update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Queued,
            None,
            None,
            output_json,
            None,
            Some("retry_queued".into()),
            message,
        )
        .await?;
    ctx.db
        .update_workflow_run_status(
            ctx.workflow_run.id,
            WorkflowStatus::Waiting,
            Some(ctx.node.id.clone()),
            None,
            Some(format!(
                "Retrying node {} attempt {} of {} after {} second(s)",
                ctx.node.id,
                next_attempt,
                ctx.node.retry.max_attempts,
                delay.num_seconds()
            )),
        )
        .await?;
    let event = NewOrchestrationEvent::new(
        ctx.workflow_run.id,
        Some(ctx.node.id.clone()),
        "node_retry_scheduled",
        runinator_models::json!({
            "node_id": ctx.node.id,
            "workflow_node_run_id": node_run.id,
            "attempt": next_attempt,
            "max_attempts": ctx.node.retry.max_attempts,
            "backoff_seconds": delay.num_seconds(),
        }),
    )
    .for_cursor(ctx.cursor.id);
    ctx.db
        .enqueue_ready_node(event, ctx.node.id.clone(), ready_at)
        .await?;
    Ok(())
}

/// arm a wake for one thread of control at `ready_at`.
///
/// stamping the cursor is what lets a fan-out's branches each hold a live ready row for the same
/// node: the supersede-on-arm rule narrows to the cursor, so re-arming one branch no longer silently
/// cancels its sibling's pending wake.
/// enqueue a ready row targeting one cursor.
///
/// `ready_at` lets a caller both defer (a timeout deadline) and drive immediately (`Utc::now()`, as
/// an interrupt resume does) through the one path that arms a cursor-targeted wake.
pub(super) async fn arm_cursor_wake<T: RuntimeStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    node_id: &str,
    reason: &str,
    payload: Value,
    ready_at: DateTime<Utc>,
) -> Result<(), SendableError> {
    let event =
        NewOrchestrationEvent::new(workflow_run_id, Some(node_id.to_string()), reason, payload)
            .for_cursor(cursor_id);
    db.enqueue_ready_node(event, node_id.to_string(), ready_at)
        .await?;
    Ok(())
}

pub(super) async fn ensure_node_run<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    prev_node_run_id: Option<Uuid>,
) -> Result<WorkflowNodeRun, SendableError> {
    if let Some(latest) = ctx.latest {
        return Ok(latest.clone());
    }
    ctx.db
        .create_workflow_node_run(
            ctx.workflow_run.id,
            ctx.node.id.clone(),
            ctx.node.parameters.clone().into(),
            prev_node_run_id,
            Some(ctx.cursor),
        )
        .await
}

/// like [`ensure_node_run`], but a *stale* latest — one this cursor already left and came back past
/// — yields a fresh run instead of being reused.
///
/// a node re-entered once per loop lap otherwise keeps overwriting a single row. That loses the
/// per-lap history, and for anything that bounds a search by "my own last settled run" it collapses
/// the bound, so the previous lap's work leaks into the current one. `join` is the case that forced
/// this: its lap bound is its own last settle, which never advances while the row is recycled.
pub(super) async fn ensure_node_run_for_visit<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    prev_node_run_id: Option<Uuid>,
) -> Result<WorkflowNodeRun, SendableError> {
    let current = ctx
        .latest
        .filter(|run| !super::context::is_reentry_stale(run, ctx.node_runs, ctx.cursor));
    if let Some(latest) = current {
        return Ok(latest.clone());
    }
    ctx.db
        .create_workflow_node_run(
            ctx.workflow_run.id,
            ctx.node.id.clone(),
            ctx.node.parameters.clone().into(),
            prev_node_run_id,
            Some(ctx.cursor),
        )
        .await
}

pub(super) async fn ensure_completed_node_run<T: RuntimeStore>(
    ctx: &NodeStepContext<'_, T>,
    reason: &str,
) -> Result<(), SendableError> {
    if ctx
        .latest
        .is_some_and(|run| run.status == WorkflowStatus::Succeeded)
    {
        return Ok(());
    }
    let node_run = ensure_node_run(ctx, None).await?;
    ctx.db
        .update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Succeeded,
            Some(node_run.attempt + 1),
            None,
            None,
            None,
            Some(reason.into()),
            None,
        )
        .await
}
