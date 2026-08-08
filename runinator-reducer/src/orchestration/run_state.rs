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
        let mut state = WorkflowRunState::from_state(&run.state);
        // where the cursor sits now, so a drained run keeps reporting the node it finished on.
        let settled_at = state
            .cursor(cursor_id)
            .map(|cursor| cursor.node_id().to_string())
            .or_else(|| run.active_node_id.clone());

        let fails = status.is_terminal() && status != WorkflowStatus::Succeeded;
        match (&movement, fails) {
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

        // a successful retirement that leaves siblings behind keeps the run running.
        let run_status = if !fails
            && status.is_terminal()
            && matches!(movement, CursorMove::Retire)
            && !state.cursors.is_empty()
        {
            WorkflowStatus::Running
        } else {
            status
        };
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
                state.to_state(),
                message.clone(),
            )
            .await?
        {
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
        let mut state = WorkflowRunState::from_state(&run.state);
        let mut forked = Vec::with_capacity(branches.len());
        for branch in branches {
            forked.push((state.fork_cursor(branch, forked_by), branch.clone()));
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
                state.to_state(),
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
        let mut state = WorkflowRunState::from_state(&run.state);
        mutate(&mut state);
        let encoded = state.to_state();
        if db
            .update_workflow_run_state_cas(workflow_run_id, run.state_version, encoded)
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
