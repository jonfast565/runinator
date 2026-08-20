use super::context::runtime_context;
use super::execution::{NodeStepContext, RunStepContext, WorkflowRunContext};
use super::*;
use super::{
    action, approval, assert, audit, await_run, barrier, basic, checkpoint, circuit_breaker,
    collect, compensation, control_flow, debounce, event_source, gate, input, map, mutex, output,
    signal, subflow, throttle, transform, transitions, wait,
};
use crate::machine::DriveRequest;
use crate::machine::{DriveOutcome, Suspension};
use uuid::Uuid;

const MAX_INLINE_WORKFLOW_STEPS: usize = 64;

#[tracing::instrument(
    skip_all,
    fields(run_id = %request.workflow_run_id, cursor_id = ?request.cursor_id)
)]
pub(crate) async fn drive_cursor<T: RuntimeStore>(
    db: &T,
    request: &DriveRequest,
) -> Result<DriveOutcome, SendableError> {
    let Some(mut workflow_run) = db.fetch_workflow_run(request.workflow_run_id).await? else {
        tracing::warn!("ready node references a workflow run that no longer exists");
        return Ok(DriveOutcome::Idle);
    };
    if workflow_run.status == WorkflowStatus::Queued {
        let node_id = request
            .node_hint
            .clone()
            .or_else(|| {
                request.cursor_id.and_then(|id| {
                    workflow_run
                        .execution_state
                        .cursor(id)
                        .map(|cursor| cursor.node_id().to_string())
                })
            })
            .or_else(|| workflow_run.active_node_id.clone());
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            node_id.clone(),
            None,
            Some("Workflow run claimed from ready queue".into()),
        )
        .await?;
        workflow_run.status = WorkflowStatus::Running;
        workflow_run.active_node_id = node_id;
    }

    // a drive follows one cursor from the ready row that woke it to wherever that thread of control
    // settles. picking it once and holding it is what keeps a fan-out's branches from stealing each
    // other's position when several are live.
    let mut driving: Option<Uuid> = None;
    for step in 0..MAX_INLINE_WORKFLOW_STEPS {
        let before = WorkflowProgressKey::from_run(db, workflow_run.id, driving).await?;
        let disposition =
            process_workflow_run_step(db, workflow_run.clone(), request, &mut driving).await?;
        let Some(next_run) = db.fetch_workflow_run(workflow_run.id).await? else {
            return Ok(DriveOutcome::Idle);
        };
        let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
        let after = WorkflowProgressKey::from_parts(&next_run, &node_runs, driving);
        // both of these judge the run against the thread of control this drive follows. reading
        // `active_node_id` instead judges the primary cursor's position while driving another, which
        // stops a live branch after a single step whenever a sibling is parked on an action.
        let driving_node_id = driving_position(&next_run, driving);
        let next_run_ctx = WorkflowRunContext::new(db, &next_run);
        let awaits_worker =
            active_node_awaits_worker(&next_run_ctx, driving_node_id.as_deref()).await?;
        if disposition == ReadyNodeDisposition::KeepClaim
            || should_stop_inline_progress(
                &next_run,
                &node_runs,
                driving_node_id.as_deref(),
                awaits_worker,
            )
            || before == after
        {
            tracing::debug!(
                inline_steps = step + 1,
                disposition = ?disposition,
                active_node_id = ?next_run.active_node_id,
                status = ?next_run.status,
                "workflow run step settled"
            );
            transitions::maybe_wake_subflow_parent(&next_run_ctx).await?;
            // the run reaches a *successful* terminal only when its last cursor retires (see
            // `run_state::advance_cursor`), so this gate already means "every thread of control is
            // done" — a finished fan-out branch leaves the run `Running` and does not fire these.
            // a failing terminal drains every cursor, which is the other way to get here.
            // a run that acquired a named mutex holds it for the rest of the run; release on any
            // terminal state so the next waiter can acquire. no-op for runs holding no lease.
            if next_run.status.is_terminal() {
                mutex::MutexOps::new(db)
                    .release_run_mutexes(next_run.id)
                    .await?;
                // wake any `await workflow` nodes parked on a run of this workflow (by correlation).
                transitions::maybe_wake_awaiters(&next_run_ctx).await?;
                // start any workflows chained to this one via on_success/on_failure/on_complete. this
                // also propagates the owning pipeline_run_id onto in-pipeline chained children.
                chaining::maybe_start_chained_workflows(&next_run_ctx).await?;
                // start any pipelines chained to this workflow run (chained-to-pipeline triggers).
                pipeline_orchestration::maybe_start_chained_pipelines(&next_run_ctx).await?;
                // settle the owning pipeline run if the whole member graph is now terminal.
                pipeline_orchestration::maybe_settle_pipeline_run(&next_run_ctx).await?;
            }
            let suspension = if disposition == ReadyNodeDisposition::Complete
                && !next_run.status.is_terminal()
            {
                suspension_for_position(db, &next_run, driving_node_id.as_deref()).await?
            } else {
                Suspension::External
            };
            return Ok(match disposition {
                ReadyNodeDisposition::KeepClaim => DriveOutcome::KeepClaim,
                ReadyNodeDisposition::Complete if next_run.status == WorkflowStatus::Paused => {
                    DriveOutcome::Paused
                }
                ReadyNodeDisposition::Complete
                    if next_run.status.is_terminal()
                        || driving
                            .and_then(|id| next_run.execution_state.cursor(id))
                            .is_none() =>
                {
                    DriveOutcome::Retired
                }
                ReadyNodeDisposition::Complete => DriveOutcome::Suspended(suspension),
            });
        }
        workflow_run = next_run;
    }

    tracing::warn!(
        max_inline_steps = MAX_INLINE_WORKFLOW_STEPS,
        active_node_id = ?workflow_run.active_node_id,
        "inline workflow progress limit exhausted; blocking run"
    );
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Blocked,
        workflow_run.active_node_id.clone(),
        None,
        Some("Inline workflow progress limit exhausted".into()),
    )
    .await?;
    Ok(DriveOutcome::Blocked)
}

async fn suspension_for_position<T: RuntimeStore>(
    db: &T,
    run: &WorkflowRun,
    node_id: Option<&str>,
) -> Result<Suspension, SendableError> {
    let Some(node_id) = node_id else {
        return Ok(Suspension::External);
    };
    let workflow = match run.workflow_snapshot.clone() {
        Some(workflow) => workflow,
        None => match db.fetch_workflow(run.workflow_id).await? {
            Some(workflow) => workflow,
            None => return Ok(Suspension::External),
        },
    };
    Ok(workflow
        .definition
        .nodes
        .iter()
        .find(|node| node.id == node_id)
        .map(|node| Suspension::for_node(&node.kind))
        .unwrap_or(Suspension::External))
}

/// where the cursor this drive follows currently sits, falling back to the run's mirrored primary
/// for a drive that has not resolved a cursor yet.
pub(super) fn driving_position(run: &WorkflowRun, driving: Option<Uuid>) -> Option<String> {
    let state = run.execution_state.clone();
    driving
        .and_then(|id| state.cursor(id))
        .map(|cursor| cursor.node_id().to_string())
        .or_else(|| run.active_node_id.clone())
}

/// Persist the stable identity for this cursor's current node visit.
/// A conflicting writer means the snapshot is stale, so retry the whole operation.
async fn ensure_cursor_visit<T: RuntimeStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
) -> Result<bool, SendableError> {
    for _ in 0..16 {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Ok(false);
        };
        let Some(cursor) = run.execution_state.cursor(cursor_id) else {
            return Ok(false);
        };
        let node_run_id = match cursor.node_run_id {
            Some(id) => Some(id),
            None => db
                .fetch_workflow_node_runs(workflow_run_id)
                .await?
                .into_iter()
                .rev()
                .find(|node_run| {
                    node_run.cursor_id == Some(cursor_id)
                        && node_run.node_id == cursor.node_id()
                        && !node_run.status.is_terminal()
                })
                .map(|node_run| node_run.id),
        };
        if cursor.visit_id.is_some() && cursor.node_run_id == node_run_id {
            return Ok(false);
        }

        let mut state = run.execution_state.clone();
        let Some(cursor) = state.cursor_mut(cursor_id) else {
            return Ok(false);
        };
        cursor.ensure_visit();
        if let Some(node_run_id) = node_run_id {
            cursor.attach_node_run(node_run_id);
        }
        if db
            .update_workflow_run_execution_state_cas(workflow_run_id, run.state_version, state)
            .await?
        {
            return Ok(true);
        }
    }
    Err(
        std::io::Error::other("cursor visit could not be persisted after repeated state conflicts")
            .into(),
    )
}

/// the cursor this drive advances, with a stable identity so its frames survive across drives.
///
/// a run that has never been placed gets one seeded at `start`. a linear run's list is reconciled
/// against `active_node_id`, which every transition already writes; that keeps the single-cursor
/// fast path at one write per transition while still giving the position an id.
async fn resolve_cursor<T: RuntimeStore>(
    ctx: &WorkflowRunContext<'_, T>,
    start: &str,
    request: &DriveRequest,
    driving: Option<Uuid>,
) -> Result<Option<RunCursor>, SendableError> {
    let workflow_run = ctx.workflow_run;
    let state = workflow_run.execution_state.clone();
    // already following a cursor in this drive: stay on it wherever it has moved to. debugger and
    // interrupt cursors deliberately retire into one surviving thread, so an unambiguous survivor
    // is a valid inline handoff. multiple survivors are a fan-out: choosing among them would let a
    // concurrently retired race loser execute a contender from the next loop lap.
    if let Some(id) = driving {
        if let Some(cursor) = state.cursor(id) {
            return Ok(Some(cursor.clone()));
        }
        if let Some(cursor) = request.cursor_id.and_then(|id| state.cursor(id)) {
            return Ok(Some(cursor.clone()));
        }
        return Ok(if state.cursors.len() == 1 {
            state.primary_cursor().cloned()
        } else {
            None
        });
    }
    // the ready row names the thread of control it was armed for. this is what lets one branch be
    // woken without disturbing its siblings, and what lets two cursors share a node. once an
    // addressed cursor retires, the row is stale and must not fall back by node id: a later fan-out
    // may have a different cursor at the same node.
    if let Some(id) = request.cursor_id {
        return Ok(state.cursor(id).cloned());
    }
    // a forked run has several live positions, and a row armed before wakes carried a cursor names
    // which one only by the node it was armed for.
    if state.cursors.len() > 1
        && let Some(cursor) = request
            .node_hint
            .as_deref()
            .and_then(|node_id| state.cursor_at(node_id))
    {
        return Ok(Some(cursor.clone()));
    }
    let node_id = RunCursor::resolve(workflow_run, start).into_node_id();
    if let Some(cursor) = state.primary_cursor()
        && cursor.is_at(&node_id)
    {
        return Ok(Some(cursor.clone()));
    }
    let placed = run_state::mutate_run_state(ctx.db, workflow_run.id, |state| {
        state.ensure_cursor(&node_id);
        if let Some(primary) = state.cursors.first_mut() {
            primary.move_to(node_id.clone());
        }
    })
    .await?;
    Ok(Some(
        placed
            .primary_cursor()
            .cloned()
            .unwrap_or_else(|| RunCursor::at(node_id)),
    ))
}

async fn process_workflow_run_step<T: RuntimeStore>(
    db: &T,
    mut workflow_run: WorkflowRun,
    request: &DriveRequest,
    driving: &mut Option<Uuid>,
) -> Result<ReadyNodeDisposition, SendableError> {
    if workflow_run.status.is_terminal() || workflow_run.status == WorkflowStatus::Paused {
        return Ok(ReadyNodeDisposition::Complete);
    }
    let workflow = match workflow_run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => db
            .fetch_workflow(workflow_run.workflow_id)
            .await?
            .ok_or_else(|| crate::errors::WORKFLOW_NOT_FOUND.error(workflow_run.workflow_id))?,
    };
    let (start, nodes) = runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let all_node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
    // resolve where this drive is: the run's persisted position, or its start node when the run has
    // not been placed yet. a linear run keeps `active_node_id` as the truth and the cursor list as
    // a mirror, so the position is reconciled onto the primary cursor here rather than written back
    // on every transition; a forked run's list becomes authoritative in its own right.
    let resolved = {
        let workflow_run_ctx = WorkflowRunContext::new(db, &workflow_run);
        resolve_cursor(&workflow_run_ctx, &start, request, *driving).await?
    };
    let Some(mut cursor) = resolved else {
        tracing::debug!(
            run_id = %workflow_run.id,
            cursor_id = ?(*driving).or(request.cursor_id),
            "drive addressed to a retired cursor; ignoring"
        );
        return Ok(ReadyNodeDisposition::Complete);
    };
    *driving = Some(cursor.id);
    // A cursor is the durable continuation. Stamp each graph-node visit before interpreting it so
    // duplicate drives and restarts agree on the identity of the step they are advancing.
    if (cursor.visit_id.is_none() || cursor.node_run_id.is_none())
        && ensure_cursor_visit(db, workflow_run.id, cursor.id).await?
    {
        if let Some(refreshed) = db.fetch_workflow_run(workflow_run.id).await? {
            cursor = refreshed
                .execution_state
                .cursor(cursor.id)
                .cloned()
                .unwrap_or(cursor);
            workflow_run = refreshed;
        }
    }
    // a thread frozen behind an interrupt is not driven. this sits before every other branch on
    // purpose: a suspended map child would otherwise finalize itself below, and a suspended cursor
    // would be a legal answer for the `cursor_at(node_id)` fallback in `resolve_cursor`. armed wakes
    // that land here are simply dropped — the resume path enqueues a fresh drive, and every parking
    // handler re-arms what it needs on its next visit.
    if cursor.is_suspended() {
        tracing::debug!(
            run_id = %workflow_run.id,
            node_id = %cursor,
            "drive for a cursor suspended by an interrupt; ignoring"
        );
        return Ok(ReadyNodeDisposition::Complete);
    }
    let run_state_snapshot = workflow_run.execution_state.clone();
    // what this thread of control may see. a real cursor never reads a speculative branch's output;
    // a speculative one reads its own subtree shadowing the real run. filtering once here isolates
    // every node operation — and the join's satisfaction check with them — without any of them knowing.
    // computed per drive rather than cached: it is a pure function of the definition, and a run
    // with no interrupt handlers gets an empty set for the cost of one metadata lookup.
    let region_nodes = runinator_workflows::interrupt_region_nodes(&workflow, &nodes);
    let node_runs =
        context::visible_node_runs(&cursor, &run_state_snapshot, &all_node_runs, &region_nodes);
    let workflow_run_ctx = WorkflowRunContext::new(db, &workflow_run);
    let run_step_ctx = RunStepContext::new(workflow_run_ctx, &cursor, &node_runs);
    // a map fan-out child stops when its body returns to the controlling map node, instead of
    // re-entering the map and fanning out again. finalize the child so it wakes the parent.
    if let Some(child) = run_state_snapshot.map_child.clone()
        && cursor.is_at(&child.stop_node)
    {
        map::finalize_map_child(&run_step_ctx, child).await?;
        return Ok(ReadyNodeDisposition::Complete);
    }
    // workflow-level `watch` guards: re-evaluated on every drive (including while parked), so a
    // state change a fixed checkpoint would miss still pre-empts the active node and jumps to the
    // handler. fires at most once per run.
    if let Some(handler) = evaluate_watches(&run_step_ctx, &workflow).await? {
        tracing::info!(active_node_id = %cursor, handler = %handler, "watch guard fired");
        run_state::mutate_run_state(db, workflow_run.id, |state| state.watch_fired = true).await?;
        run_state::advance_cursor(
            db,
            workflow_run.id,
            cursor.id,
            WorkflowStatus::Running,
            run_state::CursorMove::To(handler.clone()),
            Some(format!("watch guard fired; jumping to {handler}")),
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }
    let Some(node) = nodes.iter().find(|node| cursor.is_at(&node.id)) else {
        tracing::error!(active_node_id = %cursor, "active workflow node is missing from the graph");
        // a failing terminal ends the run and drains every branch; going through `advance_cursor`
        // is what applies that rule, rather than leaving a finished run holding live cursors.
        run_state::advance_cursor(
            db,
            workflow_run.id,
            cursor.id,
            WorkflowStatus::Failed,
            run_state::CursorMove::Retire,
            Some("Active workflow node is missing".into()),
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    };
    let latest = context::latest_node_run(&node_runs, cursor.node_id()).cloned();
    let step_ctx = NodeStepContext::new(run_step_ctx, &workflow, node, latest.as_ref(), &nodes);
    if node.skipped {
        basic::skip_node(&step_ctx).await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    // enforce the reentry safety bound at runtime: a `while`/`until`/poll loop header (or any
    // reentry-enabled node forming a bounded cycle) that has already been visited `max_visits` times
    // exits via `on_exhausted` instead of looping again. without this a loop whose condition never
    // goes false would spin forever, parking on each iteration. only checked when entering the node
    // fresh, never while a prior visit is still in flight.
    if reentry_exhausted(node, cursor.id, &node_runs) {
        match node.reentry.on_exhausted.as_ref() {
            Some(target) => {
                tracing::info!(
                    node_id = %node.id,
                    max_visits = node.reentry.max_visits,
                    target = target.as_str(),
                    "reentry max_visits exhausted; exiting to on_exhausted target"
                );
                run_state::advance_cursor(
                    db,
                    workflow_run.id,
                    cursor.id,
                    WorkflowStatus::Running,
                    run_state::CursorMove::To(target.as_str().to_string()),
                    Some(format!("reentry_exhausted:{}", node.id)),
                )
                .await?;
            }
            None => {
                tracing::warn!(
                    node_id = %node.id,
                    max_visits = node.reentry.max_visits,
                    "reentry max_visits exhausted with no on_exhausted target; blocking node"
                );
                transitions::block_node(
                    &step_ctx,
                    "Reentry max_visits exhausted with no on_exhausted target",
                )
                .await?;
            }
        }
        return Ok(ReadyNodeDisposition::Complete);
    }

    // the debugger gate sits here, after every branch that pre-empts the node entirely: pausing
    // "before" a node that is never going to execute strands the session on a step no command can
    // clear. a speculative cursor's externally-visible nodes are shadowed rather than dispatched.
    match debug::debug_gate(&step_ctx).await? {
        debug::DebugGate::Park => return Ok(ReadyNodeDisposition::Complete),
        debug::DebugGate::Shadow => {
            debug::shadow_node(&step_ctx).await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        debug::DebugGate::Proceed => {}
    }

    let ctx = step_ctx;

    // interrupts sit after the debugger gate — a paused thread should stay paused rather than be
    // diverted — and before dispatch, because the point is to run the handler *instead of* letting
    // this node settle. every refusal inside is silent, so a run with no handler declared reaches
    // dispatch on exactly the path it always did.
    if interrupt::InterruptOps::new(db).maybe_raise(&ctx).await? {
        return Ok(ReadyNodeDisposition::Complete);
    }

    tracing::debug!(node_id = %node.id, kind = ?node.kind, "dispatching to node operation");
    let disposition = match &node.kind {
        WorkflowNodeKind::Start => basic::StartOp.process(&ctx).await?,
        WorkflowNodeKind::Interrupt => basic::InterruptOp.process(&ctx).await?,
        WorkflowNodeKind::Action => action::ActionOp.process(&ctx).await?,
        WorkflowNodeKind::Invocation => invocation::InvocationOp.process(&ctx).await?,
        WorkflowNodeKind::Wait => wait::WaitOp.process(&ctx).await?,
        WorkflowNodeKind::Condition => basic::ConditionOp.process(&ctx).await?,
        WorkflowNodeKind::Switch => basic::SwitchOp.process(&ctx).await?,
        WorkflowNodeKind::Toggle => basic::ToggleOp.process(&ctx).await?,
        WorkflowNodeKind::Percentage => basic::PercentageOp.process(&ctx).await?,
        WorkflowNodeKind::Output => output::OutputOp.process(&ctx).await?,
        WorkflowNodeKind::Input => input::InputOp.process(&ctx).await?,
        WorkflowNodeKind::Config => basic::ConfigOp.process(&ctx).await?,
        WorkflowNodeKind::End => basic::EndOp.process(&ctx).await?,
        WorkflowNodeKind::Fail => compensation::FailOp.process(&ctx).await?,
        WorkflowNodeKind::Loop => control_flow::LoopOp.process(&ctx).await?,
        WorkflowNodeKind::Parallel => control_flow::ParallelOp.process(&ctx).await?,
        WorkflowNodeKind::Join => control_flow::JoinOp.process(&ctx).await?,
        WorkflowNodeKind::Map => map::MapOp.process(&ctx).await?,
        WorkflowNodeKind::Race => control_flow::RaceOp.process(&ctx).await?,
        WorkflowNodeKind::Try => control_flow::TryOp.process(&ctx).await?,
        WorkflowNodeKind::Approval => approval::ApprovalOp.process(&ctx).await?,
        WorkflowNodeKind::Gate => gate::GateOp.process(&ctx).await?,
        WorkflowNodeKind::Signal => signal::SignalOp.process(&ctx).await?,
        WorkflowNodeKind::Subflow => subflow::SubflowOp.process(&ctx).await?,
        WorkflowNodeKind::Assert => assert::AssertOp.process(&ctx).await?,
        WorkflowNodeKind::Transform => transform::TransformOp.process(&ctx).await?,
        WorkflowNodeKind::Audit => audit::AuditOp.process(&ctx).await?,
        WorkflowNodeKind::Checkpoint => checkpoint::CheckpointOp.process(&ctx).await?,
        WorkflowNodeKind::Mutex => mutex::MutexOp.process(&ctx).await?,
        WorkflowNodeKind::Throttle => throttle::ThrottleOp.process(&ctx).await?,
        WorkflowNodeKind::Cooldown => cooldown::CooldownOp.process(&ctx).await?,
        WorkflowNodeKind::AwaitRun => await_run::AwaitRunOp.process(&ctx).await?,
        WorkflowNodeKind::Debounce => debounce::DebounceOp.process(&ctx).await?,
        WorkflowNodeKind::Collect => collect::CollectOp.process(&ctx).await?,
        WorkflowNodeKind::Barrier => barrier::BarrierOp.process(&ctx).await?,
        WorkflowNodeKind::CircuitBreaker => circuit_breaker::CircuitBreakerOp.process(&ctx).await?,
        WorkflowNodeKind::EventSource => event_source::EventSourceOp.process(&ctx).await?,
        WorkflowNodeKind::Resume => interrupt::ResumeOp.process(&ctx).await?,
    };
    Ok(disposition)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct WorkflowProgressKey {
    status: WorkflowStatus,
    active_node_id: Option<String>,
    node_count: usize,
    latest_active_node_run_id: Option<Uuid>,
    latest_active_node_status: Option<WorkflowStatus>,
    paused: bool,
}

impl WorkflowProgressKey {
    async fn from_run<T: RuntimeStore>(
        db: &T,
        workflow_run_id: Uuid,
        driving: Option<Uuid>,
    ) -> Result<Self, SendableError> {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Err(crate::errors::WORKFLOW_RUN_NOT_FOUND.error(workflow_run_id));
        };
        let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
        Ok(Self::from_parts(&run, &nodes, driving))
    }

    // progress is measured for the cursor this drive follows, not for the run: a sibling branch
    // advancing is not progress for *this* thread of control, and must not keep its loop spinning.
    fn from_parts(
        workflow_run: &WorkflowRun,
        node_runs: &[WorkflowNodeRun],
        driving: Option<Uuid>,
    ) -> Self {
        let state = workflow_run.execution_state.clone();
        let position = driving
            .and_then(|id| state.cursor(id))
            .map(|cursor| cursor.node_id().to_string())
            .or_else(|| workflow_run.active_node_id.clone());
        let latest_active = position
            .as_deref()
            .and_then(|active| context::latest_node_run(node_runs, active));
        Self {
            status: workflow_run.status,
            active_node_id: position,
            node_count: node_runs.len(),
            latest_active_node_run_id: latest_active.map(|run| run.id),
            latest_active_node_status: latest_active.map(|run| run.status),
            // a debugger park is progress on the step that creates it and a fixpoint on the next, so
            // the inline loop exits on the parking step rather than wasting an iteration.
            paused: driving.is_some_and(|id| state.cursor_debug(id).paused),
        }
    }
}

// completed visits to a reentry-enabled node. each visit records exactly one node run for the node,
// and the bound is only consulted when entering fresh (no in-flight run), so every counted run is a
// finished iteration.
fn reentry_visits(node: &WorkflowNode, cursor_id: Uuid, node_runs: &[WorkflowNodeRun]) -> i64 {
    node_runs
        .iter()
        .filter(|run| {
            run.node_id == node.id
                && run
                    .cursor_id
                    .is_none_or(|run_cursor| run_cursor == cursor_id)
        })
        .count() as i64
}

// true when a reentry-bounded node should exit via its safety bound instead of looping again. only
// fires on a fresh entry (no in-flight run for the node), so an iteration still awaiting a worker is
// never abandoned mid-flight.
pub(super) fn reentry_exhausted(
    node: &WorkflowNode,
    cursor_id: Uuid,
    node_runs: &[WorkflowNodeRun],
) -> bool {
    let entering_fresh = node_runs
        .iter()
        .filter(|run| {
            run.node_id == node.id
                && run
                    .cursor_id
                    .is_none_or(|run_cursor| run_cursor == cursor_id)
        })
        .max_by_key(|run| run.id)
        .is_none_or(|run| run.status.is_terminal());
    entering_fresh
        && node.reentry.enabled
        && node.kind != WorkflowNodeKind::Loop
        && node
            .iteration_limit()
            .is_some_and(|limit| limit > 0 && reentry_visits(node, cursor_id, node_runs) >= limit)
}

fn should_stop_inline_progress(
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
    driving_node_id: Option<&str>,
    active_node_awaits_worker: bool,
) -> bool {
    if workflow_run.status.is_terminal()
        || matches!(
            workflow_run.status,
            WorkflowStatus::DebugPaused
                | WorkflowStatus::Paused
                | WorkflowStatus::Waiting
                | WorkflowStatus::ApprovalRequired
                | WorkflowStatus::Blocked
        )
    {
        return true;
    }

    // a re-entrant control node (loop/map/race/parallel) keeps its node-run `Running` while it
    // iterates or fans out; that is not a park, so the inline loop must keep processing it. only an
    // action node with a `Running` run is genuinely waiting on a worker that will not complete inline.
    if !active_node_awaits_worker {
        return false;
    }
    let Some(active_node_id) = driving_node_id else {
        return false;
    };
    context::latest_node_run(node_runs, active_node_id).is_some_and(|run| {
        matches!(
            run.status,
            WorkflowStatus::Running | WorkflowStatus::Waiting | WorkflowStatus::ApprovalRequired
        )
    })
}

/// evaluate the workflow's `metadata.watches` guards against the live run context. returns the
/// handler node id of the first guard whose condition holds, or `None`. skips evaluation once a
/// guard has already fired (`state.watch_fired`) and never redirects to the node already active.
async fn evaluate_watches<T: RuntimeStore>(
    ctx: &RunStepContext<'_, T>,
    workflow: &runinator_models::workflows::WorkflowDefinition,
) -> Result<Option<String>, SendableError> {
    let Some(watches) = workflow
        .definition
        .metadata
        .pointer("/watches")
        .and_then(|value| value.as_array())
    else {
        return Ok(None);
    };
    if watches.is_empty() || ctx.workflow_run.execution_state.watch_fired {
        return Ok(None);
    }
    let context = runtime_context(ctx).await;
    for watch in watches {
        let (Some(condition), Some(handler)) = (
            watch.get("condition"),
            watch.get("handler").and_then(|value| value.as_str()),
        ) else {
            continue;
        };
        if ctx.cursor.is_at(handler) {
            continue;
        }
        if runinator_workflows::evaluate_condition(condition, &context).unwrap_or(false) {
            return Ok(Some(handler.to_string()));
        }
    }
    Ok(None)
}

/// true when the thread of control this drive follows sits on an action node, the one node kind that
/// parks the run `Running` awaiting a worker result that will not arrive inline. control nodes
/// re-enter inline instead.
async fn active_node_awaits_worker<T: RuntimeStore>(
    ctx: &WorkflowRunContext<'_, T>,
    driving_node_id: Option<&str>,
) -> Result<bool, SendableError> {
    let Some(active_node_id) = driving_node_id else {
        return Ok(false);
    };
    let workflow = match ctx.workflow_run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => match ctx.db.fetch_workflow(ctx.workflow_run.workflow_id).await? {
            Some(workflow) => workflow,
            None => return Ok(false),
        },
    };
    let Ok((_, nodes)) = runinator_workflows::validate_workflow(&workflow) else {
        return Ok(false);
    };
    Ok(nodes
        .iter()
        .find(|node| node.id == active_node_id)
        .is_some_and(|node| {
            matches!(
                node.kind,
                WorkflowNodeKind::Action | WorkflowNodeKind::Invocation
            )
        }))
}
