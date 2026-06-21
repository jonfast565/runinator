use super::context::runtime_context;
use super::transitions::{
    arm_node_timeout, block_node, ensure_node_run, start_try_phase, time_out, timed_out,
    transition_from_node,
};
use super::*;

pub(super) async fn process_loop_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let context = runtime_context(db, workflow_run, node_runs).await;
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let items = runinator_workflows::parse_loop_items(&parameters).items;
    let prior_iterations = node_runs
        .iter()
        .filter(|run| run.node_id == node.id && run.status == WorkflowStatus::Succeeded)
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
            db.create_workflow_node_run(workflow_run.id, node.id.clone(), parameters.clone())
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
        // clear loop bookkeeping before exiting so the loop frame does not survive into the exit
        // path and route a downstream node back into the loop.
        let mut state = WorkflowRunState::from_state(&workflow_run.state);
        state.loop_frame = None;
        db.update_workflow_run_status(
            workflow_run.id,
            workflow_run.status,
            workflow_run.active_node_id.clone(),
            Some(state.to_state()),
            None,
        )
        .await?;
        transition_from_node(
            db,
            workflow_run,
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
    // a fresh state intentionally drops sibling frames so the loop body re-enters cleanly.
    let state = WorkflowRunState {
        loop_frame: Some(LoopFrame {
            index,
            item: items[index as usize].clone(),
            return_to: node.id.clone(),
        }),
        ..WorkflowRunState::default()
    };
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(return_to),
        Some(state.to_state()),
        None,
    )
    .await
}

pub(super) async fn process_parallel_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Running && timed_out(node, node_run) {
            return time_out(
                db,
                workflow_run,
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
    let Some(first) = params.branches.first().cloned() else {
        return block_node(db, workflow_run, node, "Parallel node has no branches").await;
    };
    let branches = params
        .branches
        .iter()
        .map(|branch| branch.as_str().to_string())
        .collect::<Vec<_>>();
    let remaining = branches.iter().skip(1).cloned().collect::<Vec<_>>();
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let output = ParallelOutput {
        branches,
        outputs: Vec::new(),
    };
    let mut state = WorkflowRunState::from_state(&workflow_run.state);
    state.parallel = Some(ParallelFrame {
        node_id: node.id.clone(),
        remaining,
    });
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
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(first.into_string()),
        Some(state.to_state()),
        None,
    )
    .await
}

pub(super) async fn process_join_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
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
        let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
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
            node,
            node_run,
            "Join node timed out",
            node_runs,
        )
        .await;
    }
    // dispatch the next parallel branch the matching parallel node fanned out, if any.
    let mut state = WorkflowRunState::from_state(&workflow_run.state);
    if let Some(target) = state
        .parallel
        .as_mut()
        .and_then(|frame| frame.pop_remaining())
    {
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(target),
            Some(state.to_state()),
            Some("join_waiting_for_parallel_branch".into()),
        )
        .await?;
        return Ok(());
    }
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
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
    arm_node_timeout(db, workflow_run.id, node).await
}

pub(super) async fn process_race_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_race_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    if node_run.status == WorkflowStatus::Running && timed_out(node, &node_run) {
        return time_out(
            db,
            workflow_run,
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
    if let Some(winner) = race_winner(&branches, params.winner, node_runs) {
        // the race is decided: mark any still-running losing branch as canceled so its node run is
        // terminal and the ws drive path can signal the worker to stop wasted work (the v1 limitation
        // noted in the README). branches that never started or already settled are left untouched.
        cancel_losing_race_branches(db, &branches, &winner, node_runs).await?;
        let output = RaceOutput {
            output: latest_succeeded_output_for(node_runs, &winner),
            winner,
        };
        transition_from_node(
            db,
            workflow_run,
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
    let mut state = WorkflowRunState::from_state(&workflow_run.state);
    let race_owned = state
        .race
        .as_ref()
        .is_some_and(|frame| frame.node_id == node.id);
    let next_target = if race_owned {
        state.race.as_mut().and_then(|frame| frame.pop_remaining())
    } else {
        let remaining = branches.iter().skip(1).cloned().collect::<Vec<_>>();
        state.race = Some(RaceFrame {
            node_id: node.id.clone(),
            remaining,
        });
        branches.first().cloned()
    };
    if let Some(target) = next_target {
        db.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Running,
            Some(node_run.attempt + 1),
            None,
            None,
            None,
            Some("race_branch_started".into()),
            None,
        )
        .await?;
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(target),
            Some(state.to_state()),
            None,
        )
        .await?;
        return Ok(());
    }
    transition_from_node(
        db,
        workflow_run,
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

pub(super) async fn process_try_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_try_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    if node_run.status == WorkflowStatus::Running && timed_out(node, &node_run) {
        return time_out(
            db,
            workflow_run,
            node,
            &node_run,
            "Try node timed out",
            node_runs,
        )
        .await;
    }
    let frame = WorkflowRunState::from_state(&workflow_run.state)
        .try_frame
        .clone()
        .unwrap_or_else(|| TryFrame {
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
        _ => block_node(db, workflow_run, node, "Try node has invalid phase").await,
    }
}

// cancel the latest non-terminal node run of each losing race branch. marking it `Canceled` makes
// the run record consistent immediately; the ws drive path then publishes a node-run-targeted worker
// control so an in-flight execution actually stops.
async fn cancel_losing_race_branches<T: DatabaseImpl>(
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
