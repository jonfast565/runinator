use super::context::runtime_context;
use super::transitions::{
    arm_node_timeout, block_node, ensure_node_run, start_try_phase, time_out, timed_out,
    transition_from_node,
};
use super::*;

pub(super) async fn process_loop_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let context = runtime_context(db, workflow_run, cursor, node_runs).await;
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let items = runinator_workflows::parse_loop_items(&parameters).items;
    // iterations belong to this thread of control. two branches looping over the same node would
    // otherwise each count the other's laps and exit early.
    let prior_iterations = node_runs
        .iter()
        .filter(|run| {
            run.node_id == node.id
                && run.status == WorkflowStatus::Succeeded
                && run.cursor_id.is_none_or(|id| id == cursor.id)
        })
        .count() as i64;
    // an expression cap is resolved into the parameters; fall back to the typed field.
    let max_iterations = parameters
        .get("max_iterations")
        .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
        .or(node.max_iterations)
        .unwrap_or(i64::MAX)
        .max(0);
    let index = prior_iterations;
    let exhausted = index >= items.len() as i64 || index >= max_iterations;
    let last = if exhausted && prior_iterations > 0 {
        latest_succeeded_output_excluding(node_runs, &node.id)
    } else {
        None
    };
    // each iteration gets its own run so prior_iterations advances. reuse the latest only if it was
    // left running from a prior interrupted visit.
    let node_run = match latest.filter(|run| run.status == WorkflowStatus::Running) {
        Some(latest) => {
            if timed_out(node, latest) {
                return time_out(
                    db,
                    workflow_run,
                    cursor,
                    node,
                    latest,
                    "Loop node timed out",
                    node_runs,
                )
                .await;
            }
            latest.clone()
        }
        None => {
            db.create_workflow_node_run(
                workflow_run.id,
                node.id.clone(),
                parameters.clone(),
                super::context::most_recently_finished_node_run(node_runs),
                Some(cursor),
            )
            .await?
        }
    };
    let output = if exhausted {
        LoopOutput {
            index,
            item: None,
            has_next: false,
            count: items.len(),
            last,
        }
    } else {
        LoopOutput {
            index,
            item: Some(items[index as usize].clone()),
            has_next: true,
            count: items.len(),
            last: None,
        }
    };
    let output_value = output.to_wire_value()?;
    let reason = if exhausted {
        "loop_exhausted"
    } else {
        "loop_iteration"
    };
    // mark the iteration succeeded so prior_iterations advances on re-entry from the loop body.
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        Some(node_run.attempt + 1),
        None,
        Some(output_value.clone()),
        None,
        Some(reason.into()),
        None,
    )
    .await?;

    if exhausted {
        // clear this cursor's loop bookkeeping before exiting, so the frame does not survive into
        // the exit path and route a downstream node back into the loop.
        run_state::mutate_cursor(db, workflow_run.id, cursor.id, |cursor| {
            cursor.loop_frame = None;
        })
        .await?;
        transition_from_node(
            db,
            workflow_run,
            cursor,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output_value),
            Some("loop_exhausted".into()),
            node_runs,
        )
        .await?;
        return Ok(());
    }

    let return_to = node
        .transitions
        .next
        .as_ref()
        .map(|target| target.as_str().to_string())
        .unwrap_or_else(|| node.id.clone());
    // reset only this thread of control's frames so the body re-enters cleanly. run-scoped state
    // and any sibling branch's frames are deliberately untouched — resetting the whole run state
    // here is what used to discard every other frame the run was tracking.
    let frame = LoopFrame {
        index,
        item: items[index as usize].clone(),
        return_to: node.id.clone(),
    };
    run_state::mutate_cursor(db, workflow_run.id, cursor.id, move |cursor| {
        cursor.clear_frames();
        cursor.loop_frame = Some(frame.clone());
    })
    .await?;
    run_state::advance_cursor(
        db,
        workflow_run.id,
        cursor.id,
        WorkflowStatus::Running,
        run_state::CursorMove::To(return_to),
        None,
    )
    .await
}

/// fan a `parallel` node out into one cursor per branch.
///
/// every branch starts at once. the previous design ran the first branch and queued the rest on a
/// run-scoped frame, so "parallel" branches actually executed one after another; a cursor per branch
/// is what makes them concurrent. the forking cursor retires — its thread of control *becomes* the
/// branches — and the matching join settles when they arrive.
pub(super) async fn process_parallel_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Running && timed_out(node, node_run) {
            return time_out(
                db,
                workflow_run,
                cursor,
                node,
                node_run,
                "Parallel node timed out",
                node_runs,
            )
            .await;
        }
        // branches dispatched; the join node settles when they complete.
        return Ok(());
    }
    let params = runinator_workflows::parse_parallel_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    if params.branches.is_empty() {
        return block_node(
            db,
            workflow_run,
            cursor,
            node,
            "Parallel node has no branches",
        )
        .await;
    }
    let branches = params
        .branches
        .iter()
        .map(|branch| branch.as_str().to_string())
        .collect::<Vec<_>>();
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
            super::context::most_recently_finished_node_run(node_runs),
            Some(cursor),
        )
        .await?;
    let output = ParallelOutput {
        branches: branches.clone(),
        outputs: Vec::new(),
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        Some(node_run.attempt + 1),
        None,
        Some(output.to_wire_value()?),
        None,
        Some("parallel_started".into()),
        None,
    )
    .await?;
    let forked =
        run_state::fork_cursors(db, workflow_run.id, cursor.id, &node.id, &branches).await?;
    for (branch_cursor, branch) in forked {
        enqueue_branch(
            db,
            workflow_run.id,
            branch_cursor,
            &branch,
            &node.id,
            "parallel_branch_started",
        )
        .await?;
    }
    Ok(())
}

/// wake a freshly forked branch on its own cursor.
///
/// the ready row carries the cursor id, so two branches entering the same node stay distinguishable
/// and re-arming one never supersedes the other's pending wake.
async fn enqueue_branch<T: ReducerStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor_id: Uuid,
    branch: &str,
    forked_by: &str,
    reason: &str,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(branch.to_string()),
        reason,
        runinator_models::json!({
            "branch": branch,
            "forked_by": forked_by,
            "cursor_id": cursor_id,
        }),
    )
    .for_cursor(cursor_id);
    db.enqueue_ready_node(event, branch.to_string(), Utc::now())
        .await?;
    Ok(())
}

/// settle a `join` when the branches it waits on have arrived.
///
/// satisfaction is read from **node runs**, not from the cursor list: a branch counts once its work
/// is recorded, whether or not its cursor is still live. because `visible_node_runs` already hid any
/// speculative output before this handler ran, a "what if" fork can never satisfy a real join.
pub(super) async fn process_join_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_join_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let wait_for = params
        .wait_for
        .iter()
        .map(|target| target.as_str().to_string())
        .collect::<Vec<_>>();
    if join_satisfied(&wait_for, params.mode, node_runs) {
        let node_run = ensure_node_run(
            db,
            workflow_run,
            cursor,
            node,
            latest,
            super::context::most_recently_finished_node_run(node_runs),
        )
        .await?;
        let output = JoinOutput {
            outputs: wait_for
                .iter()
                .filter_map(|target| latest_succeeded_output_for(node_runs, target))
                .collect(),
            wait_for,
            mode: branch_policy_name(params.mode).to_string(),
        };
        transition_from_node(
            db,
            workflow_run,
            cursor,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("join_satisfied".into()),
            node_runs,
        )
        .await?;
        return Ok(());
    }
    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting)
        && timed_out(node, node_run)
    {
        return time_out(
            db,
            workflow_run,
            cursor,
            node,
            node_run,
            "Join node timed out",
            node_runs,
        )
        .await;
    }
    // a speculative fork reaching a join stops here: it must not wait for real branches, and it must
    // not be counted as one. the fork's purpose ends where the real graph reconverges.
    if cursor.is_speculative() {
        return run_state::advance_cursor(
            db,
            workflow_run.id,
            cursor.id,
            WorkflowStatus::Succeeded,
            run_state::CursorMove::Retire,
            Some("speculative_join_reached".into()),
        )
        .await;
    }
    // an early-arriving branch retires instead of parking. the last branch to arrive is the one that
    // finds the join satisfied above and carries the run onward, so exactly one cursor leaves a join.
    // the `real_cursors` count is what stops a genuinely-alone branch retiring itself into a stall
    // just because a speculative fork happens to be live.
    let state = WorkflowRunState::from_state(&workflow_run.state);
    if cursor.forked_by.is_some() && state.real_cursors().count() > 1 {
        return run_state::advance_cursor(
            db,
            workflow_run.id,
            cursor.id,
            WorkflowStatus::Succeeded,
            run_state::CursorMove::Retire,
            Some("join_branch_arrived".into()),
        )
        .await;
    }
    let node_run = ensure_node_run(
        db,
        workflow_run,
        cursor,
        node,
        latest,
        super::context::most_recently_finished_node_run(node_runs),
    )
    .await?;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        None,
        Some("join_waiting".into()),
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
    arm_node_timeout(db, workflow_run.id, cursor, node).await
}

/// run every contender of a `race` at once and take the first to finish.
///
/// contenders fan out on their own cursors, exactly like `parallel`. running them one after another
/// — as the run-scoped frame did — made a race a race in name only: the first branch always won
/// because no other branch had started.
pub(super) async fn process_race_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_race_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_run = ensure_node_run(
        db,
        workflow_run,
        cursor,
        node,
        latest,
        super::context::most_recently_finished_node_run(node_runs),
    )
    .await?;
    if node_run.status == WorkflowStatus::Running && timed_out(node, &node_run) {
        return time_out(
            db,
            workflow_run,
            cursor,
            node,
            &node_run,
            "Race node timed out",
            node_runs,
        )
        .await;
    }
    let branches = params
        .branches
        .iter()
        .map(|branch| branch.as_str().to_string())
        .collect::<Vec<_>>();
    if branches.is_empty() {
        return block_node(db, workflow_run, cursor, node, "Race node has no branches").await;
    }

    // the race is already settled and this is a straggler arriving late: its thread of control ends
    // here rather than transitioning a second time.
    if node_run.status.is_terminal() && cursor.forked_by.as_deref() == Some(node.id.as_str()) {
        return run_state::advance_cursor(
            db,
            workflow_run.id,
            cursor.id,
            WorkflowStatus::Succeeded,
            run_state::CursorMove::Retire,
            Some("race_branch_lost".into()),
        )
        .await;
    }

    if let Some(winner) = race_winner(&branches, params.winner, node_runs) {
        // the race is decided: mark any still-running losing branch as canceled so its node run is
        // terminal and the ws drive path can signal the worker to stop wasted work. branches that
        // never started or already settled are left untouched.
        cancel_losing_race_branches(db, &branches, &winner, node_runs).await?;
        // retire the losing branches' cursors too, so they cannot carry the run past the race.
        let losing = {
            let state = WorkflowRunState::from_state(&workflow_run.state);
            state
                .cursors_forked_by(&node.id)
                .filter(|contender| contender.id != cursor.id)
                .map(|contender| contender.id)
                .collect::<Vec<_>>()
        };
        for loser in losing {
            run_state::advance_cursor(
                db,
                workflow_run.id,
                loser,
                WorkflowStatus::Succeeded,
                run_state::CursorMove::Retire,
                Some("race_branch_lost".into()),
            )
            .await?;
        }
        let output = RaceOutput {
            output: latest_succeeded_output_for(node_runs, &winner),
            winner,
        };
        transition_from_node(
            db,
            workflow_run,
            cursor,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("race_won".into()),
            node_runs,
        )
        .await?;
        return Ok(());
    }

    // first visit: start every contender. `cursors_forked_by` is the "have I already fanned out"
    // test, so a re-entry while contenders are still running falls through and simply waits.
    let already_started = {
        let state = WorkflowRunState::from_state(&workflow_run.state);
        state.cursors_forked_by(&node.id).next().is_some()
    };
    if !already_started {
        db.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Running,
            Some(node_run.attempt + 1),
            None,
            None,
            None,
            Some("race_branches_started".into()),
            None,
        )
        .await?;
        let forked =
            run_state::fork_cursors(db, workflow_run.id, cursor.id, &node.id, &branches).await?;
        for (branch_cursor, branch) in forked {
            enqueue_branch(
                db,
                workflow_run.id,
                branch_cursor,
                &branch,
                &node.id,
                "race_branch_started",
            )
            .await?;
        }
        return Ok(());
    }

    // contenders are live but none has won yet: this thread of control has nothing to do until one
    // does, and the branch cursors carry the work.
    if cursor.forked_by.as_deref() != Some(node.id.as_str()) {
        return Ok(());
    }
    transition_from_node(
        db,
        workflow_run,
        cursor,
        node,
        &node_run,
        WorkflowStatus::Failed,
        None,
        Some("Race completed without a winning branch".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) async fn process_try_node<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_try_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_run = ensure_node_run(
        db,
        workflow_run,
        cursor,
        node,
        latest,
        super::context::most_recently_finished_node_run(node_runs),
    )
    .await?;
    if node_run.status == WorkflowStatus::Running && timed_out(node, &node_run) {
        return time_out(
            db,
            workflow_run,
            cursor,
            node,
            &node_run,
            "Try node timed out",
            node_runs,
        )
        .await;
    }
    // the phase belongs to this thread of control: two branches inside one try region would
    // otherwise share a phase and each would observe the other's.
    let frame = cursor.try_frame.clone().unwrap_or_else(|| TryFrame {
        node_id: node.id.clone(),
        phase: "body".into(),
        pending_status: None,
        pending_output: None,
    });
    let phase = frame.phase.clone();
    if latest.is_none() {
        return start_try_phase(
            db,
            workflow_run,
            cursor,
            &node_run,
            node,
            params.body.as_str(),
            "body",
            None,
            None,
        )
        .await;
    }
    match phase.as_str() {
        "body" => {
            let Some(status) = latest_status(params.body.as_str(), node_runs) else {
                return Ok(());
            };
            let body_output = latest_succeeded_output_excluding(node_runs, &node.id);
            if status == WorkflowStatus::Succeeded {
                if let Some(finally) = params.finally {
                    return start_try_phase(
                        db,
                        workflow_run,
                        cursor,
                        &node_run,
                        node,
                        finally.as_str(),
                        "finally",
                        Some(status),
                        body_output,
                    )
                    .await;
                }
                transition_from_node(
                    db,
                    workflow_run,
                    cursor,
                    node,
                    &node_run,
                    status,
                    body_output,
                    Some("try_body_succeeded".into()),
                    node_runs,
                )
                .await?;
                return Ok(());
            }
            if let Some(catch) = params.catch {
                return start_try_phase(
                    db,
                    workflow_run,
                    cursor,
                    &node_run,
                    node,
                    catch.as_str(),
                    "catch",
                    Some(status),
                    None,
                )
                .await;
            }
            if let Some(finally) = params.finally {
                return start_try_phase(
                    db,
                    workflow_run,
                    cursor,
                    &node_run,
                    node,
                    finally.as_str(),
                    "finally",
                    Some(status),
                    body_output,
                )
                .await;
            }
            transition_from_node(
                db,
                workflow_run,
                cursor,
                node,
                &node_run,
                status,
                body_output,
                Some("try_body_failed".into()),
                node_runs,
            )
            .await?;
            Ok(())
        }
        "catch" => {
            let Some(status) = params
                .catch
                .as_ref()
                .and_then(|catch| latest_status(catch.as_str(), node_runs))
            else {
                return Ok(());
            };
            let catch_output = latest_succeeded_output_excluding(node_runs, &node.id);
            if let Some(finally) = params.finally {
                return start_try_phase(
                    db,
                    workflow_run,
                    cursor,
                    &node_run,
                    node,
                    finally.as_str(),
                    "finally",
                    Some(status),
                    catch_output,
                )
                .await;
            }
            transition_from_node(
                db,
                workflow_run,
                cursor,
                node,
                &node_run,
                status,
                catch_output,
                Some("try_catch_completed".into()),
                node_runs,
            )
            .await?;
            Ok(())
        }
        "finally" => {
            let Some(finally) = params.finally.as_ref().map(|target| target.as_str()) else {
                return Ok(());
            };
            if latest_status(finally, node_runs).is_none() {
                return Ok(());
            }
            let status = frame.pending_status.unwrap_or(WorkflowStatus::Succeeded);
            transition_from_node(
                db,
                workflow_run,
                cursor,
                node,
                &node_run,
                status,
                frame.pending_output,
                Some("try_finally_completed".into()),
                node_runs,
            )
            .await?;
            Ok(())
        }
        _ => block_node(db, workflow_run, cursor, node, "Try node has invalid phase").await,
    }
}

// cancel the latest non-terminal node run of each losing race branch. marking it `Canceled` makes
// the run record consistent immediately; the ws drive path then publishes a node-run-targeted worker
// control so an in-flight execution actually stops.
async fn cancel_losing_race_branches<T: ReducerStore>(
    db: &T,
    branches: &[String],
    winner: &str,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    for branch in branches {
        if branch == winner {
            continue;
        }
        let Some(node_run) = node_runs
            .iter()
            .rev()
            .find(|run| run.node_id == *branch && !run.status.is_terminal())
        else {
            continue;
        };
        db.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Canceled,
            None,
            None,
            None,
            None,
            Some("race_branch_canceled".into()),
            Some("Canceled because another race branch won".into()),
        )
        .await?;
    }
    Ok(())
}

fn latest_succeeded_output_for(node_runs: &[WorkflowNodeRun], node_id: &str) -> Option<Value> {
    node_runs
        .iter()
        .rev()
        .find(|run| run.node_id == node_id && run.status == WorkflowStatus::Succeeded)
        .and_then(|run| run.output_json.clone())
}

fn latest_succeeded_output_excluding(
    node_runs: &[WorkflowNodeRun],
    node_id: &str,
) -> Option<Value> {
    node_runs
        .iter()
        .rev()
        .find(|run| run.node_id != node_id && run.status == WorkflowStatus::Succeeded)
        .and_then(|run| run.output_json.clone())
}

pub(super) struct LoopHandler;
pub(super) struct ParallelHandler;
pub(super) struct JoinHandler;
pub(super) struct RaceHandler;
pub(super) struct TryHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for LoopHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_loop_node(
                ctx.db,
                ctx.workflow_run,
                ctx.cursor,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: ReducerStore> super::handler::NodeHandler<T> for ParallelHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_parallel_node(
                ctx.db,
                ctx.workflow_run,
                ctx.cursor,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: ReducerStore> super::handler::NodeHandler<T> for JoinHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_join_node(
                ctx.db,
                ctx.workflow_run,
                ctx.cursor,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: ReducerStore> super::handler::NodeHandler<T> for RaceHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_race_node(
                ctx.db,
                ctx.workflow_run,
                ctx.cursor,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}

impl<T: ReducerStore> super::handler::NodeHandler<T> for TryHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_try_node(
                ctx.db,
                ctx.workflow_run,
                ctx.cursor,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}
