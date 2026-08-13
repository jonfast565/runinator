// read-modify-write of the run state blob under the optimistic-concurrency guard.
//
// every frame mutation reads the whole `WorkflowRunState`, changes one part of it, and writes the
// whole thing back. with a single cursor per run that was safe by construction, because nothing
// drove two of a run's ready nodes at once. plural cursors remove that guarantee, so a mutation
// that must not lose a concurrent writer's frames goes through here instead.

use super::*;

/// how many times a losing writer rebuilds its change on top of the winner before giving up. a
/// conflict means another cursor of the same run wrote first; it is resolved by re-reading, not by
/// waiting, so the bound only needs to cover a burst of concurrent cursors.
const MAX_STATE_CAS_ATTEMPTS: usize = 8;

/// what happens to a cursor when its node settles.
#[derive(Debug, Clone)]
pub(super) enum CursorMove {
    /// this thread of control continues at the named node.
    To(String),
    /// this thread of control is finished; the run ends when the last one retires.
    Retire,
}

/// settle one cursor: move it or retire it, and apply the run status that goes with it, in a single
/// guarded write.
///
/// the run only takes a *successful* terminal once its last cursor retires — a fan-out is still
/// running while any branch is. a failing terminal is different: it ends the whole run immediately
/// and drains every cursor, because no branch should keep going after the run has failed.
pub(super) async fn advance_cursor<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    status: WorkflowStatus,
    movement: CursorMove,
    message: Option<String>,
) -> Result<(), SendableError> {
    for attempt in 0..MAX_STATE_CAS_ATTEMPTS {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Ok(());
        };
        let mut state = run.execution_state.clone();
        // where the cursor sits now, so a drained run keeps reporting the node it finished on.
        let settled_at = state
            .cursor(cursor_id)
            .map(|cursor| cursor.node_id().to_string())
            .or_else(|| run.active_node_id.clone());
        // read before mutating: a drain removes the cursor this question is about.
        let speculative = state.is_speculative(cursor_id);
        // a handler leaving without having reached a `resume` still owes the thread it suspended its
        // release; capture the frame now, because the write below removes the cursor holding it.
        let handler_frame = state
            .cursor(cursor_id)
            .and_then(|cursor| cursor.interrupt.clone());
        let handler_node_id = state
            .cursor(cursor_id)
            .map(|cursor| cursor.node_id().to_string());
        let handler = handler_frame.is_some();

        let fails = status.is_terminal() && status != WorkflowStatus::Succeeded;

        // a suspended thread may not be *moved* — only the handler returning control moves it, and
        // it does so through `finish_interrupt` rather than here. this guard is what closes the race
        // between a worker result landing and an interrupt being installed: `transition_from_node`
        // stamps the node run and then advances, so without it a concurrent result could walk the
        // cursor off the node the interrupt just snapshotted. every settle path funnels through
        // here, so one check covers action results, timeouts, joins, races, and try phases.
        //
        // retiring is still allowed: a race loser or a failing run must be able to drain a
        // suspended cursor, and `retire_cursor` takes its handler with it.
        if matches!(movement, CursorMove::To(_))
            && state.cursor(cursor_id).is_some_and(RunCursor::is_suspended)
        {
            tracing::debug!(
                run_id = %workflow_run_id,
                cursor_id = %cursor_id,
                "refusing to move a cursor suspended by an interrupt"
            );
            return Ok(());
        }

        match (&movement, fails) {
            // a failing "what if" branch takes only its own subtree with it. draining the run would
            // let a hypothetical failure kill the real work it was forked from.
            (_, true) if speculative => {
                for id in state.speculative_subtree(cursor_id) {
                    state.retire_cursor(id);
                }
            }
            // a failing handler takes only itself. an interrupt is a side-channel, so it must not be
            // able to end the run it was observing — the thread it suspended is still valid work.
            // the caller releases that thread; here we only make sure the failure stops at the
            // handler instead of draining every cursor.
            (_, true) if handler => {
                state.retire_cursor(cursor_id);
            }
            (_, true) => state.cursors.clear(),
            (CursorMove::To(next), false) => {
                state.ensure_cursor(next);
                if let Some(cursor) = state.cursor_mut(cursor_id) {
                    cursor.move_to(next.clone());
                }
            }
            (CursorMove::Retire, false) => {
                state.retire_cursor(cursor_id);
            }
        }

        let run_status = if speculative || handler {
            // neither a speculative cursor nor an interrupt handler moves the run's status: the run
            // means what its real threads of control say it means, and both of these are commentary
            // on a thread rather than a thread themselves.
            run.status
        } else if !fails
            && status.is_terminal()
            && matches!(movement, CursorMove::Retire)
            && state.real_cursors().next().is_some()
        {
            // a successful retirement that leaves real siblings behind keeps the run running. the
            // test is on *real* cursors so an abandoned "what if" fork cannot pin a finished run open.
            WorkflowStatus::Running
        } else {
            status
        };
        // the run is settling for good: drop any speculative branches still walking, so the ready
        // queue reaper and the terminal accounting see no live cursors.
        if !speculative && !handler && run_status.is_terminal() {
            state.cursors.clear();
        }
        let position = state
            .primary_cursor()
            .map(|cursor| cursor.node_id().to_string())
            .or(settled_at);

        if db
            .update_workflow_run_status_cas(
                workflow_run_id,
                run.state_version,
                run_status,
                position,
                state,
                message.clone(),
            )
            .await?
        {
            // the handler's thread of control just ended without going through `finish_interrupt`,
            // so nothing has released the thread it suspended. do it now, as a plain resume: a
            // frozen cursor with no handler alive to free it would hang the run forever.
            if let Some(frame) = handler_frame
                && matches!(movement, CursorMove::Retire)
            {
                super::interrupt::InterruptOps::new(db)
                    .release_suspended_thread(
                        workflow_run_id,
                        &frame,
                        handler_node_id.as_deref().unwrap_or_default(),
                        message
                            .as_deref()
                            .unwrap_or("handler ended without a resume"),
                    )
                    .await?;
            }
            return Ok(());
        }
        tracing::debug!(
            run_id = %workflow_run_id,
            attempt = attempt + 1,
            "run state changed under a cursor advance; rebuilding on the winner"
        );
    }
    Err(crate::errors::RUN_STATE_CONFLICT.error(workflow_run_id))
}

/// fan out branch cursors from `forked_by`, retiring the forking cursor, in one guarded write.
/// returns the new cursors' ids paired with the branch each entered, so the caller can wake them.
pub(super) async fn fork_cursors<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    forked_by: &str,
    branches: &[String],
) -> Result<Vec<(Uuid, String)>, SendableError> {
    for attempt in 0..MAX_STATE_CAS_ATTEMPTS {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Ok(Vec::new());
        };
        let mut state = run.execution_state.clone();
        let mut forked = Vec::with_capacity(branches.len());
        for branch in branches {
            forked.push((
                state.fork_cursor(cursor_id, branch, forked_by),
                branch.clone(),
            ));
        }
        // the forking cursor's thread of control becomes the branches; it does not continue itself.
        state.retire_cursor(cursor_id);
        let position = state
            .primary_cursor()
            .map(|cursor| cursor.node_id().to_string());

        if db
            .update_workflow_run_status_cas(
                workflow_run_id,
                run.state_version,
                WorkflowStatus::Running,
                position,
                state,
                None,
            )
            .await?
        {
            return Ok(forked);
        }
        tracing::debug!(
            run_id = %workflow_run_id,
            attempt = attempt + 1,
            "run state changed under a fan-out; rebuilding on the winner"
        );
    }
    Err(crate::errors::RUN_STATE_CONFLICT.error(workflow_run_id))
}

/// park one cursor under the debugger: persist its runtime snapshot, refresh the run-scoped mirror,
/// and take `DebugPaused` only when no thread of control can still advance.
///
/// one branch stopping at a breakpoint leaves the run `Running` — its siblings are still executing.
/// this is the same shape as the rule in [`advance_cursor`] that keeps a run alive while any branch
/// remains, and it is why the debugger's ui gates on the *cursor's* state rather than the run's.
pub(super) async fn park_cursor_for_debug<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    runtime: runinator_models::workflow_state::DebugRuntime,
    message: Option<String>,
) -> Result<(), SendableError> {
    for attempt in 0..MAX_STATE_CAS_ATTEMPTS {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Ok(());
        };
        let mut state = run.execution_state.clone();
        if state.cursor(cursor_id).is_none() {
            // retired under us; nothing to park.
            return Ok(());
        }
        state.set_cursor_debug(cursor_id, runtime.clone());
        let status = if state.all_cursors_paused() {
            WorkflowStatus::DebugPaused
        } else {
            run.status
        };
        let position = state
            .primary_cursor()
            .map(|cursor| cursor.node_id().to_string())
            .or_else(|| run.active_node_id.clone());
        if db
            .update_workflow_run_status_cas(
                workflow_run_id,
                run.state_version,
                status,
                position,
                state,
                message.clone(),
            )
            .await?
        {
            return Ok(());
        }
        tracing::debug!(
            run_id = %workflow_run_id,
            attempt = attempt + 1,
            "run state changed under a debug park; rebuilding on the winner"
        );
    }
    Err(crate::errors::RUN_STATE_CONFLICT.error(workflow_run_id))
}

/// apply `mutate` to one cursor's own frames and persist it.
///
/// a cursor missing from the list has already been retired by another writer — its thread of
/// control is over, so the mutation is dropped rather than resurrecting it.
pub(super) async fn mutate_cursor<T, F>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    mutate: F,
) -> Result<(), SendableError>
where
    T: ReducerStore,
    F: Fn(&mut RunCursor),
{
    mutate_run_state(db, workflow_run_id, |state| {
        if let Some(cursor) = state.cursor_mut(cursor_id) {
            mutate(cursor);
        }
    })
    .await?;
    Ok(())
}

/// apply `mutate` to the run's state and persist it, retrying against a fresh read when another
/// writer moved the row first.
///
/// `mutate` must be replayable: it is re-run from scratch on each attempt, against whatever the
/// winning writer left behind. returns the state that was persisted.
pub(super) async fn mutate_run_state<T, F>(
    db: &T,
    workflow_run_id: Uuid,
    mutate: F,
) -> Result<WorkflowRunState, SendableError>
where
    T: ReducerStore,
    F: Fn(&mut WorkflowRunState),
{
    for attempt in 0..MAX_STATE_CAS_ATTEMPTS {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Ok(WorkflowRunState::default());
        };
        let mut state = run.execution_state.clone();
        mutate(&mut state);
        if db
            .update_workflow_run_execution_state_cas(
                workflow_run_id,
                run.state_version,
                state.clone(),
            )
            .await?
        {
            return Ok(state);
        }
        tracing::debug!(
            run_id = %workflow_run_id,
            attempt = attempt + 1,
            "run state changed under a writer; rebuilding on the winner"
        );
    }
    Err(crate::errors::RUN_STATE_CONFLICT.error(workflow_run_id))
}
