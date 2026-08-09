// interrupts: suspend a thread of control, run a handler region beside it, hand control back.
//
// the shape is deliberately the same as a speculative cursor's: a second cursor walks the graph
// next to the real ones under carve-outs that stop it deciding what the run means. the difference
// is direction — a speculative fork explores forward and is thrown away, while a handler exists to
// return, so it carries the position it must return to.
//
// everything here is fail-open. if an interrupt cannot be serviced for any reason, none is raised
// and the drive proceeds exactly as it would have without the feature; an interrupt must never be
// able to stall or break a run it cannot handle.

use runinator_models::interrupt::{
    InterruptFrame, InterruptMode, InterruptSource, ResumePoint, handled_key,
};
use runinator_workflows::interrupt_declarations;

use super::handler::{NodeHandler, NodeHandlerContext};
use super::transitions::{block_node, transition_from_node};
use super::*;

/// which interrupt source, if any, this drive represents.
///
/// exhaustive by construction: every source is one arm. adding a source means adding a variant to
/// [`InterruptSource`] and an arm here, and nothing else in this file changes.
pub(super) fn source_for_drive(
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Option<InterruptSource> {
    // `wake`: a parked node's timer elapsed. v1 binds this to a `wait` deadline, which is the one
    // park whose resumption is purely a function of the clock.
    if node.kind == WorkflowNodeKind::Wait && super::wait::deadline_elapsed(latest) {
        return Some(InterruptSource::Wake);
    }
    None
}

/// what the raising drive carries into the region as `interrupt.payload`.
fn payload_for(source: InterruptSource, latest: Option<&WorkflowNodeRun>) -> Value {
    match source {
        InterruptSource::Wake => latest
            .and_then(|run| run.state.decode::<WaitState>().ok())
            .map(|state| runinator_models::json!({ "deadline_unix": state.deadline_unix }))
            .unwrap_or(Value::Null),
    }
}

/// raise `source` against this cursor if a handler is declared and everything about the interrupt
/// is serviceable. returns whether the drive was diverted, so the caller can stop processing.
///
/// every refusal below is silent and leaves no run-visible trace: an interrupt that cannot run is
/// simply not raised.
pub(super) async fn maybe_raise<T: ReducerStore>(
    db: &T,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    nodes: &[WorkflowNode],
    latest: Option<&WorkflowNodeRun>,
) -> Result<bool, SendableError> {
    // a speculative branch must not be able to run a handler: its "what if" is not a real thread,
    // and the handler would write node runs the real cursor can see.
    if cursor.is_speculative() || cursor.is_suspended() || cursor.is_interrupt_handler() {
        return Ok(false);
    }
    if !runinator_workflows::graph_role(&node.kind).interruptible {
        return Ok(false);
    }
    let Some(source) = source_for_drive(node, latest) else {
        return Ok(false);
    };
    // the fired-interrupt record is keyed to the node run, so a plain `resume` — after which the
    // raising condition is usually still true — does not immediately raise the same interrupt again.
    let key = latest.map(|run| handled_key(source, run.id));
    if let Some(key) = key.as_deref()
        && cursor.has_handled(key)
    {
        return Ok(false);
    }

    let Some(declaration) = interrupt_declarations(workflow)
        .into_iter()
        .find(|declaration| declaration.source() == Some(source))
    else {
        return Ok(false);
    };
    let entry = declaration.handler.clone();

    // re-check the region at runtime rather than trusting import-time validation: a definition can
    // have been written by a different binary, whose allowlist is not this one.
    if !runinator_workflows::interrupt_region_is_supported(&entry, nodes) {
        tracing::warn!(
            run_id = %workflow_run.id,
            handler = %entry,
            "interrupt handler region is not supported by this binary; not raising"
        );
        return Ok(false);
    }

    let state = WorkflowRunState::from_state(&workflow_run.state);
    // an unwinding run is already running synthetic compensation work on this cursor; the two would
    // compete for it.
    if state.compensation.is_some() {
        return Ok(false);
    }

    // one id for every attempt of the compare-and-swap, so whichever attempt lands is the one the
    // ready row below is armed for.
    let handler_cursor_id = Uuid::now_v7();
    let frame = InterruptFrame {
        interrupted_cursor: cursor.id,
        source,
        payload: payload_for(source, latest),
        resume: ResumePoint {
            node_id: cursor.node_id().to_string(),
            loop_frame: cursor.loop_frame.clone(),
            try_frame: cursor.try_frame.clone(),
        },
        raised_at: Utc::now(),
    };
    let interrupted = cursor.id;
    let entry_node = entry.clone();
    let handled = key.clone();
    let persisted = run_state::mutate_run_state(db, workflow_run.id, move |state| {
        // replayable: re-derived from scratch on each attempt, so a losing writer rebuilds the same
        // suspension on top of whatever won.
        let Some(target) = state.cursor_mut(interrupted) else {
            return;
        };
        if target.is_suspended() {
            return;
        }
        target.suspended_by = Some(handler_cursor_id);
        if let Some(key) = handled.clone() {
            target.mark_handled(key);
        }
        if state.cursor(handler_cursor_id).is_none() {
            let mut handler = RunCursor::interrupt_handler(entry_node.clone(), frame.clone());
            handler.id = handler_cursor_id;
            state.cursors.push(handler);
        }
    })
    .await?;
    if persisted.cursor(handler_cursor_id).is_none() {
        // another writer suspended this cursor first, or retired it. either way there is nothing to
        // divert.
        return Ok(false);
    }

    // a run executing a handler is running. leave `active_node_id` alone — the mirror belongs to the
    // primary thread of control, not to a side-channel.
    if matches!(
        workflow_run.status,
        WorkflowStatus::Waiting | WorkflowStatus::ApprovalRequired | WorkflowStatus::DebugPaused
    ) {
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            None,
            None,
            Some(format!("interrupt '{source}' raised")),
        )
        .await?;
    }

    let event = NewOrchestrationEvent::new(
        workflow_run.id,
        Some(entry.clone()),
        "interrupt_raised",
        runinator_models::json!({
            "source": source.as_str(),
            "handler": entry,
            "interrupted_node_id": cursor.node_id(),
        }),
    )
    .for_cursor(handler_cursor_id);
    db.enqueue_ready_node(event, entry.clone(), Utc::now())
        .await?;
    tracing::info!(
        run_id = %workflow_run.id,
        source = %source,
        handler = %entry,
        interrupted_node_id = %cursor,
        "interrupt raised; suspending the thread of control"
    );
    Ok(true)
}

/// process a `resume` node: end the handler region and hand control back to the suspended thread.
pub(super) async fn process_resume_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    nodes: &[WorkflowNode],
    node_runs: &[WorkflowNodeRun],
) -> Result<ReadyNodeDisposition, SendableError> {
    // a speculative cursor that wandered into a region has no interrupt to finish; retire it the
    // way a speculative join does rather than letting it complete a real one.
    if cursor.is_speculative() {
        run_state::advance_cursor(
            db,
            workflow_run.id,
            cursor.id,
            WorkflowStatus::Succeeded,
            run_state::CursorMove::Retire,
            Some("speculative cursor reached a resume node".into()),
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }
    let Some(frame) = cursor.interrupt.clone() else {
        block_node(
            db,
            workflow_run,
            cursor,
            node,
            "Resume node reached outside an interrupt handler",
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    };
    let mode = node
        .parameters
        .get("mode")
        .and_then(Value::as_str)
        .and_then(InterruptMode::from_str)
        .unwrap_or_default();

    finish_interrupt(db, workflow_run, cursor, &frame, mode, nodes, node_runs).await?;
    Ok(ReadyNodeDisposition::Complete)
}

/// hand control back to the suspended thread and retire the handler cursor.
///
/// restoring the whole resume point rather than diffing it makes this idempotent: a duplicated
/// drive writes the position and frames it would have written the first time. a missing handler
/// cursor means another drive already finished this interrupt, so it is a no-op.
async fn finish_interrupt<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    handler: &RunCursor,
    frame: &InterruptFrame,
    mode: InterruptMode,
    nodes: &[WorkflowNode],
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let interrupted = frame.interrupted_cursor;
    let resume_node_id = frame.resume.node_id.clone();

    // the node the thread was on, and whatever run of it was left in flight.
    let resumed_node = nodes.iter().find(|node| node.id.as_str() == resume_node_id);
    let in_flight = context::latest_node_run(node_runs, &resume_node_id)
        .filter(|run| !run.status.is_terminal())
        .cloned();

    // `continue`/`restart` leave the node behind, so any run of it still sitting non-terminal has to
    // be closed out. forgetting this is what would leave `latest_node_run` returning a `Waiting` row
    // forever, hanging the thread on its next visit.
    if matches!(mode, InterruptMode::Continue | InterruptMode::Restart)
        && let Some(stale) = in_flight.as_ref()
    {
        let reason = match mode {
            InterruptMode::Restart => "interrupt_restarted",
            _ => "interrupt_skipped",
        };
        db.update_workflow_node_run(
            stale.id,
            WorkflowStatus::Canceled,
            None,
            None,
            None,
            None,
            Some(reason.into()),
            Some(format!(
                "closed by interrupt handler '{}'",
                handler.node_id()
            )),
        )
        .await?;
    }

    let point = frame.resume.clone();
    let handler_id = handler.id;
    // one guarded write: un-suspend and restore the thread, then retire the handler. doing both
    // together is what stops a crash between them leaving a frozen cursor with no handler alive to
    // release it.
    let frozen_seconds = (Utc::now() - frame.raised_at).num_seconds().max(0);
    run_state::mutate_run_state(db, workflow_run.id, move |state| {
        if let Some(target) = state.cursor_mut(interrupted) {
            target.suspended_by = None;
            target.move_to(point.node_id.clone());
            target.loop_frame = point.loop_frame.clone();
            target.try_frame = point.try_frame.clone();
            // credit the time spent frozen back to whatever deadline is measured at this position,
            // so a slow handler does not silently consume a park's window. `restart` re-enters the
            // node with a fresh node run, so its clock starts over and there is nothing to credit.
            match mode {
                InterruptMode::Restart => target.suspended_seconds = 0,
                _ => target.suspended_seconds += frozen_seconds,
            }
        }
        state.cursors.retain(|cursor| cursor.id != handler_id);
    })
    .await?;

    match mode {
        // re-enter the node. its own handler re-reads the node run and does the right thing: a
        // terminal action transitions, a park re-parks, an unstarted node executes.
        InterruptMode::Resume | InterruptMode::Restart => {
            enqueue_resume_drive(
                db,
                workflow_run.id,
                interrupted,
                &resume_node_id,
                "interrupt_resumed",
                runinator_models::json!({ "mode": mode.as_str(), "node_id": resume_node_id }),
            )
            .await?;
        }
        // settle the node and take the matching edge. `Succeeded`/`Failed` is chosen here rather
        // than inherited, because the node may never have run at all.
        InterruptMode::Continue | InterruptMode::Fail => {
            let status = if mode == InterruptMode::Fail {
                WorkflowStatus::Failed
            } else {
                WorkflowStatus::Succeeded
            };
            let Some(node) = resumed_node else {
                // the graph no longer has the node the thread was on; retire rather than strand.
                run_state::advance_cursor(
                    db,
                    workflow_run.id,
                    interrupted,
                    status,
                    run_state::CursorMove::Retire,
                    Some(format!(
                        "interrupt resumed onto missing node '{resume_node_id}'"
                    )),
                )
                .await?;
                return Ok(());
            };
            let resumed_cursor = fetch_cursor(db, workflow_run.id, interrupted).await?;
            let Some(resumed_cursor) = resumed_cursor else {
                return Ok(());
            };
            let run = db
                .fetch_workflow_run(workflow_run.id)
                .await?
                .unwrap_or_else(|| workflow_run.clone());
            let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
            // the node may never have run at all — `resume next` past a node the thread had not yet
            // entered is legitimate — so materialize a run to settle rather than assuming one.
            let node_run = match in_flight {
                Some(stale) => stale,
                None => {
                    transitions::ensure_node_run(db, &run, &resumed_cursor, node, None, None)
                        .await?
                }
            };
            transition_from_node(
                db,
                &run,
                &resumed_cursor,
                node,
                &node_run,
                status,
                None,
                Some(format!("interrupt_{}", mode.as_str())),
                &node_runs,
            )
            .await?;
        }
    }
    tracing::info!(
        run_id = %workflow_run.id,
        mode = %mode,
        resume_node_id = %resume_node_id,
        "interrupt handler returned control"
    );
    Ok(())
}

/// re-read one cursor after the state write that released it.
async fn fetch_cursor<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
) -> Result<Option<RunCursor>, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Ok(None);
    };
    Ok(WorkflowRunState::from_state(&run.state)
        .cursor(cursor_id)
        .cloned())
}

/// wake the resumed thread on its own node.
///
/// this is deliberately a fresh ready row rather than continuing inline: the drive loop follows one
/// cursor, keyed on `driving`, so switching threads mid-drive would confuse its progress detector.
/// the row doubles as the orchestration record of the return, which is why the event type and
/// payload are passed in rather than fixed.
async fn enqueue_resume_drive<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    node_id: &str,
    event_type: &str,
    payload: Value,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node_id.to_string()),
        event_type,
        payload,
    )
    .for_cursor(cursor_id);
    db.enqueue_ready_node(event, node_id.to_string(), Utc::now())
        .await?;
    Ok(())
}

/// release a thread whose handler cursor went away without reaching a `resume`.
///
/// two things land here. a handler node that failed with no `on_failure` route inside the region:
/// the interrupt was a side-channel, so its failure must not take the run with it — the thread it
/// suspended is still valid work. and a region that simply runs off the end of its graph, which the
/// validator rejects but a hand-written definition can still contain. both are treated as a plain
/// `resume`, because returning control is always safer than stranding a frozen cursor.
///
/// the handler cursor is already gone by the time this runs — [`run_state::advance_cursor`] retired
/// it in the same write that decided it was leaving.
pub(super) async fn release_suspended_thread<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    frame: &InterruptFrame,
    handler_node_id: &str,
    reason: &str,
) -> Result<(), SendableError> {
    tracing::warn!(
        run_id = %workflow_run_id,
        handler_node_id,
        reason,
        "interrupt handler ended without a resume; returning control without failing the run"
    );
    let point = frame.resume.clone();
    let interrupted = frame.interrupted_cursor;
    // a handler that died still froze the thread for as long as it ran, so the credit is owed
    // exactly as it is on the happy path.
    let frozen_seconds = (Utc::now() - frame.raised_at).num_seconds().max(0);
    run_state::mutate_run_state(db, workflow_run_id, move |state| {
        if let Some(target) = state.cursor_mut(interrupted) {
            target.suspended_by = None;
            target.move_to(point.node_id.clone());
            target.loop_frame = point.loop_frame.clone();
            target.try_frame = point.try_frame.clone();
            target.suspended_seconds += frozen_seconds;
        }
    })
    .await?;
    // the resume drive doubles as the orchestration record, so the run's event log shows both that
    // the handler ended badly and that control went back.
    enqueue_resume_drive(
        db,
        workflow_run_id,
        interrupted,
        &frame.resume.node_id,
        "interrupt_handler_failed",
        runinator_models::json!({
            "source": frame.source.as_str(),
            "reason": reason,
            "handler_node_id": handler_node_id,
            "node_id": frame.resume.node_id,
        }),
    )
    .await
}

pub(super) struct ResumeHandler;

impl<T: ReducerStore> NodeHandler<T> for ResumeHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_resume_node(
                ctx.db,
                ctx.workflow_run,
                ctx.cursor,
                ctx.node,
                ctx.nodes,
                ctx.node_runs,
            )
            .await
        }
    }
}
