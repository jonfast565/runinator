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

use super::context::is_reentry_stale;
use super::execution::{NodeExecutionContext, NodeStepContext};
use super::transitions::{block_node, timed_out, timed_out_since_created};
use super::*;

/// the transition reason stamped on a node run `resume restart` cancels. read back by
/// [`super::context::is_reentry_stale`], which is what makes the next visit a fresh one.
pub(super) const RESTARTED_REASON: &str = "interrupt_restarted";

/// a raise this drive has decided to make, carrying everything the write needs.
struct Raised {
    source: InterruptSource,
    /// what the region reads as `interrupt.payload`.
    payload: Value,
    /// the out-of-band request this came from, consumed by the same write that raises it.
    request_id: Option<Uuid>,
}

/// interrupt lifecycle operations bound to one store handle. `db` is the only parameter genuinely
/// invariant across every method below — the run, cursor, and node an operation concerns vary per
/// call (some methods don't even take a run), so those stay method arguments rather than fields.
pub(super) struct InterruptOps<'a, T: RuntimeStore> {
    db: &'a T,
}

impl<'a, T: RuntimeStore> InterruptOps<'a, T> {
    pub(super) fn new(db: &'a T) -> Self {
        Self { db }
    }

    /// does this drive represent `source`, and if so what does it carry into the region?
    ///
    /// one arm per source, each answering both questions at once: a predicate that matched and then
    /// had to re-derive its own evidence to build a payload is exactly how the two drift apart.
    /// adding a source is a variant on [`InterruptSource`], an entry in its `ALL`, and an arm here.
    ///
    /// `latest` has already had a stale re-entry filtered out, so a loop body returning to a node
    /// whose previous iteration failed does not read that iteration as a fresh failure.
    async fn detect(
        &self,
        ctx: &NodeStepContext<'_, T>,
        source: InterruptSource,
        latest: Option<&WorkflowNodeRun>,
    ) -> Result<Option<Value>, SendableError> {
        let db = self.db;
        let node = ctx.node;
        let payload = match source {
            // a parked node's timer elapsed. bound to a `wait` deadline, the one park whose resumption
            // is purely a function of the clock.
            InterruptSource::Wake => {
                if node.kind != WorkflowNodeKind::Wait || !super::wait::deadline_elapsed(latest) {
                    return Ok(None);
                }
                let deadline = latest
                    .and_then(|run| run.state.decode::<WaitState>().ok())
                    .map(|state| state.deadline_unix);
                runinator_models::json!({ "node_id": node.id, "deadline_unix": deadline })
            }
            // the node's own deadline has blown while its run is still in flight, so the thread is about
            // to be timed out. raising here rather than after the fact is the point: the handler still
            // has a live node run to decide about. an implicit default deadline is deliberately not
            // enough — only a timeout the author wrote down raises this.
            InterruptSource::Timeout => {
                let Some(run) = latest.filter(|run| !run.status.is_terminal()) else {
                    return Ok(None);
                };
                let Some(timeout) = node.timeout_seconds else {
                    return Ok(None);
                };
                // a park never goes `Running`, so its clock runs from creation; a dispatched node's runs
                // from `started_at`. reading the wrong one would fire before the node itself agrees it
                // has overrun.
                let blown = match run.status {
                    WorkflowStatus::Running => timed_out(ctx.timing(), run),
                    _ => timed_out_since_created(ctx.timing(), run),
                };
                if !blown {
                    return Ok(None);
                }
                let since = run.started_at.unwrap_or(run.created_at);
                runinator_models::json!({
                    "node_id": node.id,
                    "timeout_seconds": timeout,
                    "elapsed_seconds": (Utc::now() - since).num_seconds(),
                })
            }
            // a failed node run is queued for another attempt. the handler runs before the re-dispatch,
            // so it can fix whatever the attempt needs, or step past the node entirely.
            //
            // the retry scheduler is the only thing that leaves a node run `Queued`, so the status alone
            // would do today; the reason is checked as well so this keeps meaning "a retry" if something
            // else ever parks a run there.
            InterruptSource::Retry => {
                let Some(run) = latest.filter(|run| {
                    run.status == WorkflowStatus::Queued
                        && run.transition_reason.as_deref() == Some("retry_queued")
                }) else {
                    return Ok(None);
                };
                runinator_models::json!({
                    "node_id": node.id,
                    "attempt": run.attempt + 1,
                    "max_attempts": node.retry.max_attempts,
                    "message": run.message,
                })
            }
            // the node settled badly and the thread is about to take its failure route. a `TimedOut` run
            // lands here rather than under `timeout`, which only ever matches a run still in flight.
            InterruptSource::Failure => {
                let Some(run) = latest.filter(|run| {
                    matches!(
                        run.status,
                        WorkflowStatus::Failed | WorkflowStatus::TimedOut
                    )
                }) else {
                    return Ok(None);
                };
                runinator_models::json!({
                    "node_id": node.id,
                    "status": run.status.as_str(),
                    "message": run.message,
                    "output": run.output_json,
                })
            }
            // an out-of-band park resolution landed: an endpoint stamped the node run `Succeeded` and
            // woke the run. a polled park (`gate`) transitions inline on the poll that opens it and so
            // never produces a drive in this shape — which is what keeps a 30s poll from raising an
            // interrupt every 30s.
            InterruptSource::Resolved => {
                if !matches!(
                    node.kind,
                    WorkflowNodeKind::Signal | WorkflowNodeKind::Approval | WorkflowNodeKind::Input
                ) {
                    return Ok(None);
                }
                let Some(run) = latest.filter(|run| run.status == WorkflowStatus::Succeeded) else {
                    return Ok(None);
                };
                runinator_models::json!({
                    "node_id": node.id,
                    "kind": serde_json::to_value(&node.kind).unwrap_or_default(),
                    "output": run.output_json,
                })
            }
            // a child run this thread is parked on reached a terminal. the read is only paid for by a
            // run that both parked on a subflow and declared a handler for this source.
            InterruptSource::Child => {
                if node.kind != WorkflowNodeKind::Subflow {
                    return Ok(None);
                }
                let Some(state) = latest
                    .filter(|run| run.status == WorkflowStatus::Waiting)
                    .and_then(|run| SubflowState::from_wire_value(&run.state).ok())
                else {
                    return Ok(None);
                };
                let Some(child) = db.fetch_workflow_run(state.subflow_run_id).await? else {
                    return Ok(None);
                };
                if !child.status.is_terminal() {
                    return Ok(None);
                }
                runinator_models::json!({
                    "node_id": node.id,
                    "child_run_id": child.id,
                    "status": child.status.as_str(),
                })
            }
            // requested from outside the run: there is no node state to match, so these are raised from
            // the pending queue in `maybe_raise` and never from a drive.
            InterruptSource::External | InterruptSource::OrphanSignal => return Ok(None),
        };
        Ok(Some(payload))
    }

    /// the raise this drive should make, considering only sources a handler actually answers.
    ///
    /// filtering by declaration before matching is what keeps the feature free for a workflow that
    /// uses none of it, and what confines a predicate's database read to a run that asked for that
    /// source.
    async fn resolve_raise(
        &self,
        ctx: &NodeStepContext<'_, T>,
        declared: &[InterruptSource],
        state: &WorkflowRunState,
        latest: Option<&WorkflowNodeRun>,
    ) -> Result<Option<Raised>, SendableError> {
        let pending = state.pending_interrupt_for(ctx.cursor.id);
        for source in InterruptSource::ALL {
            if !declared.contains(&source) {
                continue;
            }
            if source.requested() {
                if let Some(request) = pending.filter(|request| request.source == source) {
                    return Ok(Some(Raised {
                        source,
                        payload: request.payload.clone(),
                        request_id: Some(request.id),
                    }));
                }
                continue;
            }
            if let Some(payload) = self.detect(ctx, source, latest).await? {
                return Ok(Some(Raised {
                    source,
                    payload,
                    request_id: None,
                }));
            }
        }
        Ok(None)
    }

    /// drop a request the drive looked at and refused, so it cannot fire at some arbitrary later point
    /// in the run. every refusal that reaches here is a standing fact about the run — no handler, an
    /// unsupported region, a node that cannot be interrupted — not a condition the next drive would
    /// answer differently.
    async fn refuse_request(
        &self,
        workflow_run_id: Uuid,
        request_id: Option<Uuid>,
        reason: &str,
    ) -> Result<(), SendableError> {
        let Some(request_id) = request_id else {
            return Ok(());
        };
        tracing::warn!(
            run_id = %workflow_run_id,
            reason,
            "an interrupt requested from outside the run cannot be serviced; dropping it"
        );
        run_state::mutate_run_state(self.db, workflow_run_id, move |state| {
            state.take_pending_interrupt(request_id);
        })
        .await?;
        Ok(())
    }

    /// raise `source` against this cursor if a handler is declared and everything about the
    /// interrupt is serviceable. returns whether the drive was diverted, so the caller can stop
    /// processing.
    ///
    /// every refusal below is silent and leaves no run-visible trace: an interrupt that cannot run
    /// is simply not raised.
    pub(super) async fn maybe_raise(
        &self,
        ctx: &NodeExecutionContext<'_, T>,
    ) -> Result<bool, SendableError> {
        // a speculative branch must not be able to run a handler: its "what if" is not a real
        // thread, and the handler would write node runs the real cursor can see. a pending request
        // is left alone here rather than refused — these cursors are not the thread it is waiting
        // for.
        if ctx.cursor.is_speculative()
            || ctx.cursor.is_suspended()
            || ctx.cursor.is_interrupt_handler()
        {
            return Ok(false);
        }
        // no handler declared for any source this binary knows, and nothing asked from outside: the
        // whole feature costs one metadata lookup and one key probe. every predicate below,
        // including the ones that read the database, is only reached by a run that asked for that
        // source.
        let declarations = interrupt_declarations(ctx.workflow, ctx.nodes);
        let declared: Vec<InterruptSource> = declarations
            .iter()
            .filter(|declaration| declaration.enabled)
            .filter_map(|declaration| declaration.source())
            .collect();
        let requested = ctx
            .workflow_run
            .state
            .get("pending_interrupts")
            .is_some_and(|value| !value.is_null());
        if declared.is_empty() && !requested {
            return Ok(false);
        }

        let state = ctx.run_state_snapshot();
        // only one interrupt handler may be live in a run at a time: `context::visible_node_runs`
        // hides a handler's region behind a single "am i a handler" boolean rather than naming
        // which region, so two concurrently-live handlers (e.g. two `parallel` branches each
        // raising a different declared source) could see each other's node runs. serializing here
        // is what keeps that assumption true rather than merely documented. this is not a permanent
        // refusal — a pending request is left alone rather than dropped, so the next drive gets a
        // fair shot once the live handler resumes.
        if state.cursors.iter().any(RunCursor::is_interrupt_handler) {
            return Ok(false);
        }
        // a request nobody answers is dropped by the drive that looks at it rather than left
        // parked: a handler that appears later in the run's life would fire it long after the
        // caller gave up.
        if let Some(request) = state.pending_interrupt_for(ctx.cursor.id)
            && !declared.contains(&request.source)
        {
            self.refuse_request(
                ctx.workflow_run.id,
                Some(request.id),
                "no handler is declared for the requested source",
            )
            .await?;
            return Ok(false);
        }
        // a node run left behind by a previous loop iteration is not evidence about this one — the
        // node handlers all filter it out before reading a status, and so must the sources.
        let latest = ctx
            .latest
            .filter(|run| !is_reentry_stale(run, ctx.node_runs, ctx.cursor));
        let Some(raised) = self.resolve_raise(ctx, &declared, state, latest).await? else {
            return Ok(false);
        };
        let source = raised.source;

        if !runinator_workflows::graph_role(&ctx.node.kind).interruptible {
            self.refuse_request(
                ctx.workflow_run.id,
                raised.request_id,
                "the thread is on a node that cannot be interrupted",
            )
            .await?;
            return Ok(false);
        }
        // the fired-interrupt record is keyed to the node run and its attempt, so a plain `resume`
        // — after which the raising condition is usually still true — does not immediately raise
        // the same interrupt again. the attempt is part of the key because a retry reuses one ctx.node
        // run row across every attempt; keying on the row alone would dedupe attempt 2 against the
        // interrupt attempt 1 already fired, silently limiting `retry` to firing once per node
        // visit instead of once per re-dispatch. a requested source is exempt: it is consumed by
        // this drive either way, so it cannot loop, and an explicit second ask at the same node
        // deserves a second handler run.
        let key = (!source.requested())
            .then(|| latest.map(|run| handled_key(source, run.id, run.attempt)))
            .flatten();
        if let Some(key) = key.as_deref()
            && ctx.cursor.has_handled(key)
        {
            return Ok(false);
        }

        let Some(declaration) = declarations
            .into_iter()
            .find(|declaration| declaration.enabled && declaration.source() == Some(source))
        else {
            return Ok(false);
        };
        let entry = declaration.handler.clone();

        // re-check the region at runtime rather than trusting import-time validation: a definition
        // can have been written by a different binary, whose allowlist is not this one.
        if !runinator_workflows::interrupt_region_is_supported(&entry, ctx.nodes) {
            tracing::warn!(
                run_id = %ctx.workflow_run.id,
                handler = %entry,
                "interrupt handler region is not supported by this binary; not raising"
            );
            self.refuse_request(
                ctx.workflow_run.id,
                raised.request_id,
                "the declared handler region is not supported by this binary",
            )
            .await?;
            return Ok(false);
        }

        // an unwinding run is already running synthetic compensation work on this cursor; the two
        // would compete for it.
        if state.compensation.is_some() {
            self.refuse_request(
                ctx.workflow_run.id,
                raised.request_id,
                "the run is unwinding compensation",
            )
            .await?;
            return Ok(false);
        }

        // one id for every attempt of the compare-and-swap, so whichever attempt lands is the one
        // the ready row below is armed for.
        let handler_cursor_id = Uuid::now_v7();
        let frame = InterruptFrame {
            interrupted_cursor: ctx.cursor.id,
            source,
            payload: raised.payload,
            resume: ResumePoint {
                node_id: ctx.cursor.node_id().to_string(),
                loops: ctx.cursor.loops.clone(),
                try_frame: ctx.cursor.try_frame.clone(),
            },
            raised_at: Utc::now(),
        };
        let interrupted = ctx.cursor.id;
        let entry_node = entry.clone();
        let handled = key.clone();
        let request_id = raised.request_id;
        let persisted = run_state::mutate_run_state(self.db, ctx.workflow_run.id, move |state| {
            // replayable: re-derived from scratch on each attempt, so a losing writer rebuilds the
            // same suspension on top of whatever won.
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
            // consuming the request in the same write that raises it is what stops a retried
            // compare-and-swap raising the same ask twice.
            if let Some(request_id) = request_id {
                state.take_pending_interrupt(request_id);
            }
        })
        .await?;
        if persisted.cursor(handler_cursor_id).is_none() {
            // another writer suspended this cursor first, or retired it. either way there is
            // nothing to divert.
            return Ok(false);
        }

        // a run executing a handler is running. leave `active_node_id` alone — the mirror belongs
        // to the primary thread of control, not to a side-channel.
        if matches!(
            ctx.workflow_run.status,
            WorkflowStatus::Waiting
                | WorkflowStatus::ApprovalRequired
                | WorkflowStatus::DebugPaused
        ) {
            self.db
                .update_workflow_run_status(
                    ctx.workflow_run.id,
                    WorkflowStatus::Running,
                    None,
                    None,
                    Some(format!("interrupt '{source}' raised")),
                )
                .await?;
        }

        let event = NewOrchestrationEvent::new(
            ctx.workflow_run.id,
            Some(entry.clone()),
            "interrupt_raised",
            runinator_models::json!({
                "source": source.as_str(),
                "handler": entry,
                "interrupted_node_id": ctx.cursor.node_id(),
            }),
        )
        .for_cursor(handler_cursor_id);
        self.db
            .enqueue_ready_node(event, entry.clone(), Utc::now())
            .await?;
        tracing::info!(
            run_id = %ctx.workflow_run.id,
            source = %source,
            handler = %entry,
            interrupted_node_id = %ctx.cursor,
            "interrupt raised; suspending the thread of control"
        );
        Ok(true)
    }

    /// process a `resume` node: end the handler region and hand control back to the suspended
    /// thread.
    pub(super) async fn reduce_resume_node(
        &self,
        ctx: &NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        // a speculative cursor that wandered into a region has no interrupt to finish; retire it
        // the way a speculative join does rather than letting it complete a real one.
        if ctx.cursor.is_speculative() {
            run_state::advance_cursor(
                self.db,
                ctx.workflow_run.id,
                ctx.cursor.id,
                WorkflowStatus::Succeeded,
                run_state::CursorMove::Retire,
                Some("speculative cursor reached a resume node".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        let Some(frame) = ctx.cursor.interrupt.clone() else {
            block_node(ctx, "Resume node reached outside an interrupt handler").await?;
            return Ok(ReadyNodeDisposition::Complete);
        };
        let mode = ctx
            .node
            .parameters
            .get("mode")
            .and_then(Value::as_str)
            .and_then(|mode| mode.parse().ok())
            .unwrap_or_default();

        self.finish_interrupt(ctx, &frame, mode).await?;
        Ok(ReadyNodeDisposition::Complete)
    }

    /// hand control back to the suspended thread and retire the handler cursor.
    ///
    /// restoring the whole resume point rather than diffing it makes this idempotent: a duplicated
    /// drive writes the position and frames it would have written the first time. a missing
    /// handler cursor means another drive already finished this interrupt, so it is a no-op.
    async fn finish_interrupt(
        &self,
        ctx: &NodeExecutionContext<'_, T>,
        frame: &InterruptFrame,
        mode: InterruptMode,
    ) -> Result<(), SendableError> {
        let db = self.db;
        let workflow = ctx.workflow;
        let workflow_run = ctx.workflow_run;
        let handler = ctx.cursor;
        let nodes = ctx.nodes;
        let node_runs = ctx.node_runs;
        let interrupted = frame.interrupted_cursor;
        let resume_node_id = frame.resume.node_id.clone();

        // the node the thread was on, and whatever run of it was left in flight.
        let resumed_node = nodes
            .iter()
            .find(|candidate| candidate.id.as_str() == resume_node_id);
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
                InterruptMode::Restart => RESTARTED_REASON,
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
        let persisted = run_state::mutate_run_state(db, workflow_run.id, move |state| {
            if let Some(target) = state.cursor_mut(interrupted) {
                target.suspended_by = None;
                target.move_to(point.node_id.clone());
                target.loops = point.loops.clone();
                target.try_frame = point.try_frame.clone();
                // credit the time spent frozen back to whatever deadline is measured at this position,
                // so a slow handler does not silently consume a park's window. `restart` re-enters the
                // node with a fresh node run, so its clock starts over and there is nothing to credit.
                match mode {
                    InterruptMode::Restart => target.suspended_seconds = 0,
                    _ => target.suspended_seconds += frozen_seconds,
                }
            }
            state.cursors.retain(|candidate| candidate.id != handler_id);
        })
        .await?;

        match mode {
            // re-enter the node. its own handler re-reads the node run and does the right thing: a
            // terminal action transitions, a park re-parks, an unstarted node executes.
            InterruptMode::Resume | InterruptMode::Restart => {
                self.enqueue_resume_drive(
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
                let Some(resumed_definition) = resumed_node else {
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
                // the write above already returned the state it persisted; re-deriving the cursor from a
                // fresh fetch would cost a round trip and reopen a window for a concurrent write to move
                // it again between the write and the read.
                let Some(resumed_cursor) = persisted.cursor(interrupted).cloned() else {
                    return Ok(());
                };
                let run = db
                    .fetch_workflow_run(workflow_run.id)
                    .await?
                    .unwrap_or_else(|| workflow_run.clone());
                let all_node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
                // filtered exactly as an ordinary dispatch would see it: the handler region's own node
                // runs must not leak into the resumed thread's `steps.*` or its edge-condition context,
                // which is the whole point of a handler being a side-channel rather than a real branch.
                let region_nodes = runinator_workflows::interrupt_region_nodes(workflow, nodes);
                let run_state = run.execution_state.clone();
                let visible_runs = context::visible_node_runs(
                    &resumed_cursor,
                    &run_state,
                    &all_node_runs,
                    &region_nodes,
                );
                // the node may never have run at all — `resume next` past a node the thread had not yet
                // entered is legitimate — so materialize a run to settle rather than assuming one. an
                // already-terminal run (e.g. the `Failed` run a `failure` interrupt raised on) must still
                // be reused rather than replaced: settling a fabricated empty run here would discard the
                // real output/message and leave a duplicate, data-less node run behind.
                let existing = context::latest_node_run(&visible_runs, &resume_node_id).cloned();
                let resumed_ctx = NodeStepContext::new(
                    super::execution::RunStepContext::new(
                        super::execution::WorkflowRunContext::new(db, &run),
                        &resumed_cursor,
                        &visible_runs,
                    ),
                    workflow,
                    resumed_definition,
                    existing.as_ref(),
                    nodes,
                );
                let node_run = transitions::ensure_node_run(&resumed_ctx, None).await?;
                // an interrupt handler's decision is explicit, not an organic dispatch result, so the
                // node's own retry policy must not be allowed to intercept it: `resume fail` means fail
                // this node now, not "try again if attempts remain".
                transitions::settle_node(
                    &resumed_ctx,
                    &node_run,
                    status,
                    None,
                    Some(format!("interrupt_{}", mode.as_str())),
                    false,
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

    /// wake the resumed thread on its own node.
    ///
    /// this is deliberately a fresh ready row rather than continuing inline: the drive loop
    /// follows one cursor, keyed on `driving`, so switching threads mid-drive would confuse its
    /// progress detector. the row doubles as the orchestration record of the return, which is why
    /// the event type and payload are passed in rather than fixed. thin wrapper over
    /// [`transitions::arm_cursor_wake`], fixing `ready_at` to now rather than a deferred deadline.
    async fn enqueue_resume_drive(
        &self,
        workflow_run_id: Uuid,
        cursor_id: Uuid,
        node_id: &str,
        event_type: &str,
        payload: Value,
    ) -> Result<(), SendableError> {
        transitions::arm_cursor_wake(
            self.db,
            workflow_run_id,
            cursor_id,
            node_id,
            event_type,
            payload,
            Utc::now(),
        )
        .await
    }

    /// release a thread whose handler cursor went away without reaching a `resume`.
    ///
    /// two things land here. a handler node that failed with no `on_failure` route inside the
    /// region: the interrupt was a side-channel, so its failure must not take the run with it —
    /// the thread it suspended is still valid work. and a region that simply runs off the end of
    /// its graph, which the validator rejects but a hand-written definition can still contain.
    /// both are treated as a plain `resume`, because returning control is always safer than
    /// stranding a frozen cursor.
    ///
    /// the handler cursor is already gone by the time this runs — [`run_state::advance_cursor`]
    /// retired it in the same write that decided it was leaving.
    pub(super) async fn release_suspended_thread(
        &self,
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
        run_state::mutate_run_state(self.db, workflow_run_id, move |state| {
            if let Some(target) = state.cursor_mut(interrupted) {
                target.suspended_by = None;
                target.move_to(point.node_id.clone());
                target.loops = point.loops.clone();
                target.try_frame = point.try_frame.clone();
                target.suspended_seconds += frozen_seconds;
            }
        })
        .await?;
        // the resume drive doubles as the orchestration record, so the run's event log shows both
        // that the handler ended badly and that control went back.
        self.enqueue_resume_drive(
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
}

pub(super) struct ResumeOp;

impl ResumeOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        InterruptOps::new(ctx.db).reduce_resume_node(ctx).await
    }
}
