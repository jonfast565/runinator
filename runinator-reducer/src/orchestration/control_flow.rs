use super::context::runtime_context;
use super::transitions::{
    arm_node_timeout, block_node, ensure_node_run, start_try_phase, time_out, timed_out,
    transition_from_node,
};
use super::*;

pub(super) struct LoopHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for LoopHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let context = runtime_context(ctx).await;
        let parameters = runinator_workflows::resolve_value_refs(&ctx.node.parameters, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let items = runinator_workflows::parse_loop_items(&parameters).items;
        // iterations belong to this thread of control. two branches looping over the same node would
        // otherwise each count the other's laps and exit early.
        let prior_iterations = ctx
            .node_runs
            .iter()
            .filter(|run| {
                run.node_id == ctx.node.id
                    && run.status == WorkflowStatus::Succeeded
                    && run.cursor_id.is_none_or(|id| id == ctx.cursor.id)
            })
            .count() as i64;
        // an expression cap is resolved into the parameters; fall back to the typed field.
        let max_iterations = parameters
            .get("max_iterations")
            .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
            .or(ctx.node.max_iterations)
            .unwrap_or(i64::MAX)
            .max(0);
        let index = prior_iterations;
        let exhausted = index >= items.len() as i64 || index >= max_iterations;
        let last = if exhausted && prior_iterations > 0 {
            latest_succeeded_output_excluding(ctx.node_runs, &ctx.node.id)
        } else {
            None
        };
        // each iteration gets its own run so prior_iterations advances. reuse the latest only if it was
        // left running from a prior interrupted visit.
        let node_run = match ctx
            .latest
            .filter(|run| run.status == WorkflowStatus::Running)
        {
            Some(latest) => {
                if timed_out(ctx.timing(), latest) {
                    return super::handler::complete(
                        time_out(ctx, latest, "Loop node timed out").await,
                    );
                }
                latest.clone()
            }
            None => {
                ctx.db
                    .create_workflow_node_run(
                        ctx.workflow_run.id,
                        ctx.node.id.clone(),
                        parameters.clone(),
                        super::context::most_recently_finished_node_run(ctx.node_runs),
                        Some(ctx.cursor),
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
        ctx.db
            .update_workflow_node_run(
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
            run_state::mutate_cursor(ctx.db, ctx.workflow_run.id, ctx.cursor.id, |cursor| {
                cursor.loop_frame = None;
            })
            .await?;
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(output_value),
                Some("loop_exhausted".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        let return_to = ctx
            .node
            .transitions
            .next
            .as_ref()
            .map(|target| target.as_str().to_string())
            .unwrap_or_else(|| ctx.node.id.clone());
        // reset only this thread of control's frames so the body re-enters cleanly. run-scoped state
        // and any sibling branch's frames are deliberately untouched — resetting the whole run state
        // here is what used to discard every other frame the run was tracking.
        let frame = LoopFrame {
            index,
            item: items[index as usize].clone(),
            return_to: ctx.node.id.clone(),
        };
        run_state::mutate_cursor(ctx.db, ctx.workflow_run.id, ctx.cursor.id, move |cursor| {
            cursor.clear_frames();
            cursor.loop_frame = Some(frame.clone());
        })
        .await?;
        super::handler::complete(
            run_state::advance_cursor(
                ctx.db,
                ctx.workflow_run.id,
                ctx.cursor.id,
                WorkflowStatus::Running,
                run_state::CursorMove::To(return_to),
                None,
            )
            .await,
        )
    }
}

/// fan a `parallel` node out into one cursor per branch.
///
/// every branch starts at once. the previous design ran the first branch and queued the rest on a
/// run-scoped frame, so "parallel" branches actually executed one after another; a cursor per branch
/// is what makes them concurrent. the forking cursor retires — its thread of control *becomes* the
/// branches — and the matching join settles when they arrive.
pub(super) struct ParallelHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for ParallelHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        if let Some(node_run) = ctx.latest {
            if node_run.status == WorkflowStatus::Running && timed_out(ctx.timing(), node_run) {
                return super::handler::complete(
                    time_out(ctx, node_run, "Parallel node timed out").await,
                );
            }
            // branches dispatched; the join node settles when they complete.
            return Ok(ReadyNodeDisposition::Complete);
        }
        let params = runinator_workflows::parse_parallel_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        if params.branches.is_empty() {
            return super::handler::complete(
                block_node(ctx, "Parallel node has no branches").await,
            );
        }
        let branches = params
            .branches
            .iter()
            .map(|branch| branch.as_str().to_string())
            .collect::<Vec<_>>();
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
        let output = ParallelOutput {
            branches: branches.clone(),
            outputs: Vec::new(),
        };
        ctx.db
            .update_workflow_node_run(
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
        let forked = run_state::fork_cursors(
            ctx.db,
            ctx.workflow_run.id,
            ctx.cursor.id,
            &ctx.node.id,
            &branches,
        )
        .await?;
        for (branch_cursor, branch) in forked {
            enqueue_branch(ctx, branch_cursor, &branch, "parallel_branch_started").await?;
        }
        Ok(ReadyNodeDisposition::Complete)
    }
}

/// wake a freshly forked branch on its own cursor.
///
/// the ready row carries the cursor id, so two branches entering the same node stay distinguishable
/// and re-arming one never supersedes the other's pending wake.
async fn enqueue_branch<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    cursor_id: Uuid,
    branch: &str,
    reason: &str,
) -> Result<(), SendableError> {
    let event = NewOrchestrationEvent::new(
        ctx.workflow_run.id,
        Some(branch.to_string()),
        reason,
        runinator_models::json!({
            "branch": branch,
            "forked_by": ctx.node.id,
            "cursor_id": cursor_id,
        }),
    )
    .for_cursor(cursor_id);
    ctx.db
        .enqueue_ready_node(event, branch.to_string(), Utc::now())
        .await?;
    Ok(())
}

/// settle a `join` when the branches it waits on have arrived.
///
/// satisfaction is read from **node runs**, not from the cursor list: a branch counts once its work
/// is recorded, whether or not its cursor is still live. because `visible_node_runs` already hid any
/// speculative output before this handler ran, a "what if" fork can never satisfy a real join.
pub(super) struct JoinHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for JoinHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let params = runinator_workflows::parse_join_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let wait_for = params
            .wait_for
            .iter()
            .map(|target| target.as_str().to_string())
            .collect::<Vec<_>>();
        if join_satisfied(&wait_for, params.mode, ctx.node_runs) {
            let node_run = ensure_node_run(
                ctx,
                super::context::most_recently_finished_node_run(ctx.node_runs),
            )
            .await?;
            let output = JoinOutput {
                outputs: wait_for
                    .iter()
                    .filter_map(|target| latest_succeeded_output_for(ctx.node_runs, target))
                    .collect(),
                wait_for,
                mode: branch_policy_name(params.mode).to_string(),
            };
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("join_satisfied".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        if let Some(node_run) = ctx
            .latest
            .filter(|run| run.status == WorkflowStatus::Waiting)
            && timed_out(ctx.timing(), node_run)
        {
            return super::handler::complete(time_out(ctx, node_run, "Join node timed out").await);
        }
        // a speculative fork reaching a join stops here: it must not wait for real branches, and it must
        // not be counted as one. the fork's purpose ends where the real graph reconverges.
        if ctx.cursor.is_speculative() {
            return super::handler::complete(
                run_state::advance_cursor(
                    ctx.db,
                    ctx.workflow_run.id,
                    ctx.cursor.id,
                    WorkflowStatus::Succeeded,
                    run_state::CursorMove::Retire,
                    Some("speculative_join_reached".into()),
                )
                .await,
            );
        }
        // an early-arriving branch retires instead of parking. the last branch to arrive is the one that
        // finds the join satisfied above and carries the run onward, so exactly one cursor leaves a join.
        // the `joinable_cursors` count is what stops a genuinely-alone branch retiring itself into a
        // stall just because a speculative fork or an interrupt handler happens to be live. neither is
        // a sibling branch; counting one as company is exactly the bug this filter exists to prevent.
        if ctx.cursor.forked_by.is_some() && ctx.run_state_snapshot().joinable_cursors().count() > 1
        {
            return super::handler::complete(
                run_state::advance_cursor(
                    ctx.db,
                    ctx.workflow_run.id,
                    ctx.cursor.id,
                    WorkflowStatus::Succeeded,
                    run_state::CursorMove::Retire,
                    Some("join_branch_arrived".into()),
                )
                .await,
            );
        }
        let node_run = ensure_node_run(
            ctx,
            super::context::most_recently_finished_node_run(ctx.node_runs),
        )
        .await?;
        ctx.db
            .update_workflow_node_run(
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
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::Waiting,
                Some(ctx.node.id.clone()),
                None,
                None,
            )
            .await?;
        super::handler::complete(arm_node_timeout(ctx).await)
    }
}

/// run every contender of a `race` at once and take the first to finish.
///
/// contenders fan out on their own cursors, exactly like `parallel`. running them one after another
/// — as the run-scoped frame did — made a race a race in name only: the first branch always won
/// because no other branch had started.
pub(super) struct RaceHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for RaceHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let params = runinator_workflows::parse_race_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let node_run = ensure_node_run(
            ctx,
            super::context::most_recently_finished_node_run(ctx.node_runs),
        )
        .await?;
        if node_run.status == WorkflowStatus::Running && timed_out(ctx.timing(), &node_run) {
            return super::handler::complete(time_out(ctx, &node_run, "Race node timed out").await);
        }
        let branches = params
            .branches
            .iter()
            .map(|branch| branch.as_str().to_string())
            .collect::<Vec<_>>();
        if branches.is_empty() {
            return super::handler::complete(block_node(ctx, "Race node has no branches").await);
        }

        // the race is already settled and this is a straggler arriving late: its thread of control ends
        // here rather than transitioning a second time.
        if node_run.status.is_terminal()
            && ctx.cursor.forked_by.as_deref() == Some(ctx.node.id.as_str())
        {
            return super::handler::complete(
                run_state::advance_cursor(
                    ctx.db,
                    ctx.workflow_run.id,
                    ctx.cursor.id,
                    WorkflowStatus::Succeeded,
                    run_state::CursorMove::Retire,
                    Some("race_branch_lost".into()),
                )
                .await,
            );
        }

        if let Some(winner) = race_winner(&branches, params.winner, ctx.node_runs) {
            // the race is decided: mark any still-running losing branch as canceled so its node run is
            // terminal and the ws drive path can signal the worker to stop wasted work. branches that
            // never started or already settled are left untouched.
            cancel_losing_race_branches(ctx, &branches, &winner).await?;
            // retire the losing branches' cursors too, so they cannot carry the run past the race.
            let losing = {
                ctx.run_state_snapshot()
                    .cursors_forked_by(&ctx.node.id)
                    .filter(|contender| contender.id != ctx.cursor.id)
                    .map(|contender| contender.id)
                    .collect::<Vec<_>>()
            };
            for loser in losing {
                run_state::advance_cursor(
                    ctx.db,
                    ctx.workflow_run.id,
                    loser,
                    WorkflowStatus::Succeeded,
                    run_state::CursorMove::Retire,
                    Some("race_branch_lost".into()),
                )
                .await?;
            }
            let output = RaceOutput {
                output: latest_succeeded_output_for(ctx.node_runs, &winner),
                winner,
            };
            transition_from_node(
                ctx,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("race_won".into()),
            )
            .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }

        // first visit: start every contender. `cursors_forked_by` is the "have I already fanned out"
        // test, so a re-entry while contenders are still running falls through and simply waits.
        let already_started = {
            ctx.run_state_snapshot()
                .cursors_forked_by(&ctx.node.id)
                .next()
                .is_some()
        };
        if !already_started {
            ctx.db
                .update_workflow_node_run(
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
            let forked = run_state::fork_cursors(
                ctx.db,
                ctx.workflow_run.id,
                ctx.cursor.id,
                &ctx.node.id,
                &branches,
            )
            .await?;
            for (branch_cursor, branch) in forked {
                enqueue_branch(ctx, branch_cursor, &branch, "race_branch_started").await?;
            }
            return Ok(ReadyNodeDisposition::Complete);
        }

        // contenders are live but none has won yet: this thread of control has nothing to do until one
        // does, and the branch cursors carry the work.
        if ctx.cursor.forked_by.as_deref() != Some(ctx.node.id.as_str()) {
            return Ok(ReadyNodeDisposition::Complete);
        }
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Failed,
            None,
            Some("Race completed without a winning branch".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

pub(super) struct TryHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for TryHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let params = runinator_workflows::parse_try_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let node_run = ensure_node_run(
            ctx,
            super::context::most_recently_finished_node_run(ctx.node_runs),
        )
        .await?;
        if node_run.status == WorkflowStatus::Running && timed_out(ctx.timing(), &node_run) {
            return super::handler::complete(time_out(ctx, &node_run, "Try node timed out").await);
        }
        // the phase belongs to this thread of control: two branches inside one try region would
        // otherwise share a phase and each would observe the other's.
        let frame = ctx.cursor.try_frame.clone().unwrap_or_else(|| TryFrame {
            node_id: ctx.node.id.clone(),
            phase: "body".into(),
            pending_status: None,
            pending_output: None,
        });
        let phase = frame.phase.clone();
        if ctx.latest.is_none() {
            return super::handler::complete(
                start_try_phase(ctx, &node_run, params.body.as_str(), "body", None, None).await,
            );
        }
        match phase.as_str() {
            "body" => {
                let Some(status) = latest_status(params.body.as_str(), ctx.node_runs) else {
                    return Ok(ReadyNodeDisposition::Complete);
                };
                let body_output = latest_succeeded_output_excluding(ctx.node_runs, &ctx.node.id);
                if status == WorkflowStatus::Succeeded {
                    if let Some(finally) = params.finally {
                        return super::handler::complete(
                            start_try_phase(
                                ctx,
                                &node_run,
                                finally.as_str(),
                                "finally",
                                Some(status),
                                body_output,
                            )
                            .await,
                        );
                    }
                    transition_from_node(
                        ctx,
                        &node_run,
                        status,
                        body_output,
                        Some("try_body_succeeded".into()),
                    )
                    .await?;
                    return Ok(ReadyNodeDisposition::Complete);
                }
                if let Some(catch) = params.catch {
                    return super::handler::complete(
                        start_try_phase(
                            ctx,
                            &node_run,
                            catch.as_str(),
                            "catch",
                            Some(status),
                            None,
                        )
                        .await,
                    );
                }
                if let Some(finally) = params.finally {
                    return super::handler::complete(
                        start_try_phase(
                            ctx,
                            &node_run,
                            finally.as_str(),
                            "finally",
                            Some(status),
                            body_output,
                        )
                        .await,
                    );
                }
                transition_from_node(
                    ctx,
                    &node_run,
                    status,
                    body_output,
                    Some("try_body_failed".into()),
                )
                .await?;
                Ok(ReadyNodeDisposition::Complete)
            }
            "catch" => {
                let Some(status) = params
                    .catch
                    .as_ref()
                    .and_then(|catch| latest_status(catch.as_str(), ctx.node_runs))
                else {
                    return Ok(ReadyNodeDisposition::Complete);
                };
                let catch_output = latest_succeeded_output_excluding(ctx.node_runs, &ctx.node.id);
                if let Some(finally) = params.finally {
                    return super::handler::complete(
                        start_try_phase(
                            ctx,
                            &node_run,
                            finally.as_str(),
                            "finally",
                            Some(status),
                            catch_output,
                        )
                        .await,
                    );
                }
                transition_from_node(
                    ctx,
                    &node_run,
                    status,
                    catch_output,
                    Some("try_catch_completed".into()),
                )
                .await?;
                Ok(ReadyNodeDisposition::Complete)
            }
            "finally" => {
                let Some(finally) = params.finally.as_ref().map(|target| target.as_str()) else {
                    return Ok(ReadyNodeDisposition::Complete);
                };
                if latest_status(finally, ctx.node_runs).is_none() {
                    return Ok(ReadyNodeDisposition::Complete);
                }
                let status = frame.pending_status.unwrap_or(WorkflowStatus::Succeeded);
                transition_from_node(
                    ctx,
                    &node_run,
                    status,
                    frame.pending_output,
                    Some("try_finally_completed".into()),
                )
                .await?;
                Ok(ReadyNodeDisposition::Complete)
            }
            _ => super::handler::complete(block_node(ctx, "Try node has invalid phase").await),
        }
    }
}

// cancel the latest non-terminal node run of each losing race branch. marking it `Canceled` makes
// the run record consistent immediately; the ws drive path then publishes a node-run-targeted worker
// control so an in-flight execution actually stops.
async fn cancel_losing_race_branches<T: ReducerStore>(
    ctx: &super::handler::NodeHandlerContext<'_, T>,
    branches: &[String],
    winner: &str,
) -> Result<(), SendableError> {
    for branch in branches {
        if branch == winner {
            continue;
        }
        let Some(node_run) = ctx
            .node_runs
            .iter()
            .rev()
            .find(|run| run.node_id == *branch && !run.status.is_terminal())
        else {
            continue;
        };
        ctx.db
            .update_workflow_node_run(
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
