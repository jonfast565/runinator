use std::collections::HashMap;

use chrono::Utc;
use runinator_comm::{ActionCommand, WireCodec};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    orchestration::{NewOrchestrationEvent, ReadyNodeRecord},
    value::Value,
    workflow_state::{
        ApprovalRecord, ApprovalState, ConfigSummary, EmitOutput, JoinOutput, LoopFrame,
        LoopOutput, MapFrame, MapOutput, ParallelFrame, ParallelOutput, RaceFrame, RaceOutput,
        SkippedOutput, SubflowOutcome, SubflowState, SwitchOutput, TryFrame, WaitElapsedOutput,
        WaitState, WorkflowContextHeader, WorkflowRunState,
    },
    workflows::{
        WorkflowAction, WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowRun,
        WorkflowStatus, WorkflowSubflowType,
    },
};
use runinator_workflows::{
    append_completed_map_item, branch_policy_name, join_satisfied, latest_status, race_winner,
};
use uuid::Uuid;

const MAX_INLINE_WORKFLOW_STEPS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadyNodeDisposition {
    Complete,
    KeepClaim,
}

pub(crate) async fn process_ready_node<T: DatabaseImpl>(
    db: &T,
    ready_node: &ReadyNodeRecord,
) -> Result<ReadyNodeDisposition, SendableError> {
    let Some(mut workflow_run) = db.fetch_workflow_run(ready_node.workflow_run_id).await? else {
        return Ok(ReadyNodeDisposition::Complete);
    };
    if workflow_run.status == WorkflowStatus::Queued {
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(ready_node.node_id.clone()),
            None,
            Some("Workflow run claimed from ready queue".into()),
        )
        .await?;
        workflow_run.status = WorkflowStatus::Running;
        workflow_run.active_node_id = Some(ready_node.node_id.clone());
    }

    for _ in 0..MAX_INLINE_WORKFLOW_STEPS {
        let before = WorkflowProgressKey::from_run(db, workflow_run.id).await?;
        let disposition = process_workflow_run_step(db, workflow_run.clone()).await?;
        let Some(next_run) = db.fetch_workflow_run(workflow_run.id).await? else {
            return Ok(ReadyNodeDisposition::Complete);
        };
        let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
        let after = WorkflowProgressKey::from_parts(&next_run, &node_runs);
        let awaits_worker = active_node_awaits_worker(db, &next_run).await?;
        if disposition == ReadyNodeDisposition::KeepClaim
            || should_stop_inline_progress(&next_run, &node_runs, awaits_worker)
            || before == after
        {
            maybe_wake_subflow_parent(db, &next_run).await?;
            return Ok(disposition);
        }
        workflow_run = next_run;
    }

    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Blocked,
        workflow_run.active_node_id.clone(),
        None,
        Some("Inline workflow progress limit exhausted".into()),
    )
    .await?;
    Ok(ReadyNodeDisposition::Complete)
}

async fn process_workflow_run_step<T: DatabaseImpl>(
    db: &T,
    workflow_run: WorkflowRun,
) -> Result<ReadyNodeDisposition, SendableError> {
    if workflow_run.status.is_terminal() || workflow_run.status == WorkflowStatus::Paused {
        return Ok(ReadyNodeDisposition::Complete);
    }
    let workflow = match workflow_run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => db
            .fetch_workflow(workflow_run.workflow_id)
            .await?
            .ok_or_else(|| {
                Box::new(RuntimeError::new(
                    "workflow.not_found".into(),
                    format!("Workflow {} not found", workflow_run.workflow_id),
                )) as SendableError
            })?,
    };
    let (start, nodes) = runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
    let active_node_id = workflow_run
        .active_node_id
        .clone()
        .unwrap_or_else(|| start.clone());
    let Some(node) = nodes.iter().find(|node| node.id == active_node_id) else {
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Failed,
            Some(active_node_id),
            None,
            Some("Active workflow node is missing".into()),
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    };
    let latest = latest_node_run(&node_runs, &active_node_id).cloned();
    if node.skipped {
        process_skipped_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    match &node.kind {
        runinator_models::workflows::WorkflowNodeKind::Start => {
            let node_run = ensure_node_run(db, &workflow_run, node, latest.as_ref()).await?;
            transition_from_node(
                db,
                &workflow_run,
                node,
                &node_run,
                WorkflowStatus::Succeeded,
                None,
                Some("start_reached".into()),
                &node_runs,
            )
            .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Action => {
            process_action_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        runinator_models::workflows::WorkflowNodeKind::Wait => {
            return process_wait_node(db, &workflow_run, node, latest.as_ref()).await;
        }
        runinator_models::workflows::WorkflowNodeKind::Condition => {
            let node_run = db
                .create_workflow_node_run(
                    workflow_run.id,
                    node.id.clone(),
                    node.parameters.clone().into(),
                )
                .await?;
            let context = runtime_context(&workflow_run, &node_runs);
            let matched = runinator_workflows::evaluate_condition(&node.condition, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let (status, reason) = if matched {
                (WorkflowStatus::Succeeded, "condition_matched")
            } else {
                (WorkflowStatus::Blocked, "condition_unmatched")
            };
            transition_from_node(
                db,
                &workflow_run,
                node,
                &node_run,
                status,
                None,
                Some(reason.into()),
                &node_runs,
            )
            .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Switch => {
            let node_run = db
                .create_workflow_node_run(
                    workflow_run.id,
                    node.id.clone(),
                    node.parameters.clone().into(),
                )
                .await?;
            let params = runinator_workflows::parse_switch_parameters(node)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let context = runtime_context(&workflow_run, &node_runs);
            let target = runinator_workflows::evaluate_switch(&params, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let output = SwitchOutput {
                target: target.clone(),
            }
            .to_wire_value()?;
            db.update_workflow_node_run(
                node_run.id,
                if target.is_some() {
                    WorkflowStatus::Succeeded
                } else {
                    WorkflowStatus::Blocked
                },
                Some(node_run.attempt + 1),
                None,
                Some(output),
                None,
                Some("switch_evaluated".into()),
                None,
            )
            .await?;
            match target {
                Some(target) => {
                    db.update_workflow_run_status(
                        workflow_run.id,
                        WorkflowStatus::Running,
                        Some(target),
                        None,
                        None,
                    )
                    .await?;
                }
                None => {
                    transition_from_node(
                        db,
                        &workflow_run,
                        node,
                        &node_run,
                        WorkflowStatus::Blocked,
                        None,
                        Some("Switch did not match a target".into()),
                        &node_runs,
                    )
                    .await?;
                }
            }
        }
        runinator_models::workflows::WorkflowNodeKind::Emit => {
            let node_run = db
                .create_workflow_node_run(
                    workflow_run.id,
                    node.id.clone(),
                    node.parameters.clone().into(),
                )
                .await?;
            let params = runinator_workflows::parse_emit_parameters(node)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let context = runtime_context(&workflow_run, &node_runs);
            let data = runinator_workflows::resolve_value_refs(&params.data, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let output = EmitOutput {
                event_type: params.event_type,
                data,
            };
            transition_from_node(
                db,
                &workflow_run,
                node,
                &node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("emit_recorded".into()),
                &node_runs,
            )
            .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Config => {
            process_config_node(db, &workflow_run, node, &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::End => {
            ensure_completed_node_run(db, &workflow_run, node, latest.as_ref(), "end_reached")
                .await?;
            db.update_workflow_run_status(
                workflow_run.id,
                WorkflowStatus::Succeeded,
                Some(node.id.clone()),
                None,
                None,
            )
            .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Fail => {
            ensure_completed_node_run(db, &workflow_run, node, latest.as_ref(), "fail_reached")
                .await?;
            db.update_workflow_run_status(
                workflow_run.id,
                WorkflowStatus::Failed,
                Some(node.id.clone()),
                None,
                Some("Workflow reached fail node".into()),
            )
            .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Loop => {
            process_loop_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Parallel => {
            process_parallel_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Join => {
            process_join_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Map => {
            process_map_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Race => {
            process_race_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Try => {
            process_try_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Approval => {
            process_approval_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Subflow => {
            if let Err(err) =
                process_subflow_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await
            {
                // a subflow error would otherwise bubble up and retry forever while the run stays
                // non-terminal. surface it as a failed node so the workflow can follow on_failure.
                let node_run = ensure_node_run(db, &workflow_run, node, latest.as_ref()).await?;
                transition_from_node(
                    db,
                    &workflow_run,
                    node,
                    &node_run,
                    WorkflowStatus::Failed,
                    None,
                    Some(format!("Subflow node {} failed: {err}", node.id)),
                    &node_runs,
                )
                .await?;
            }
        }
    }

    Ok(ReadyNodeDisposition::Complete)
}

async fn process_action_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let action = node.action.as_ref().ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow.node.action_missing".into(),
            format!("Action node {} has no action configuration", node.id),
        )) as SendableError
    })?;
    // a loop body re-entering this node sees the prior iteration's terminal run; treat it as a
    // fresh visit so the action dispatches again instead of transitioning from the stale run.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Running {
            return Ok(());
        }
        if node_run.status.is_terminal() {
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                node_run.status,
                node_run.output_json.clone(),
                node_run.message.clone(),
                node_runs,
            )
            .await?;
            return Ok(());
        }
    }

    let node_run = match latest.filter(|run| run.status == WorkflowStatus::Queued) {
        Some(node_run) => node_run.clone(),
        None => {
            db.create_workflow_node_run(
                workflow_run.id,
                node.id.clone(),
                node.parameters.clone().into(),
            )
            .await?
        }
    };
    let attempt = node_run.attempt + 1;
    let parameters = build_node_parameters(action, node, workflow_run, node_runs)?;
    let command = build_action_command(workflow_run.id, &node_run, action, parameters.clone());
    db.enqueue_action_dispatch(format!("workflow-node-run:{}", node_run.id), command)
        .await?;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(attempt),
        Some(parameters),
        None,
        None,
        Some("action_started".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(node.id.clone()),
        None,
        None,
    )
    .await
}

async fn process_wait_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Result<ReadyNodeDisposition, SendableError> {
    let params = runinator_workflows::parse_wait_parameters(node);
    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        let wait_state = serde_json::from_value::<WaitState>(node_run.state.clone().into()).ok();
        let deadline = wait_state
            .as_ref()
            .map(|state| state.deadline_unix)
            .unwrap_or(i64::MAX);
        if Utc::now().timestamp() < deadline {
            return Ok(ReadyNodeDisposition::KeepClaim);
        }
        let output = WaitElapsedOutput {
            deadline_unix: deadline,
        };
        let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
        transition_from_node(
            db,
            workflow_run,
            node,
            node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("wait_elapsed".into()),
            &node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    let deadline = Utc::now().timestamp() + params.seconds;
    let state = WaitState {
        deadline_unix: deadline,
        status: params.initial_status,
    }
    .to_wire_value()?;
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.clone()),
        Some("wait_started".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        Some(state),
        None,
    )
    .await?;
    let ready_at = chrono::DateTime::<Utc>::from_timestamp(deadline, 0).unwrap_or_else(Utc::now);
    let event = runinator_models::orchestration::NewOrchestrationEvent::new(
        workflow_run.id,
        Some(node.id.clone()),
        "node_waiting",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), ready_at)
        .await?;
    Ok(ReadyNodeDisposition::Complete)
}

async fn process_config_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let context = runtime_context(workflow_run, node_runs);
    let resolved = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let new_name = resolved.get("name").and_then(|value| match value {
        Value::Null => None,
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        other => Some(other.to_string()),
    });
    if new_name.is_some() {
        db.set_workflow_run_name(workflow_run.id, new_name.clone())
            .await?;
    }
    let summary = ConfigSummary {
        name: new_name,
        metadata: resolved.get("metadata").cloned(),
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(summary.to_wire_value()?),
        Some("config_applied".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

async fn process_skipped_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    let output = SkippedOutput {
        skipped: true,
        node_id: node.id.clone(),
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some(format!("Node {} skipped", node.id)),
        node_runs,
    )
    .await?;
    Ok(())
}

// --- rich control-flow nodes -------------------------------------------------
//
// the reducer lives here and calls `DatabaseImpl` directly. control-flow bookkeeping lives in
// named frames inside `workflow_run.state` (the typed `WorkflowRunState` from runinator-models).
// predicates that read sibling node-run history come from runinator-workflows.

async fn process_loop_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let context = runtime_context(workflow_run, node_runs);
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let items = runinator_workflows::parse_loop_items(&parameters).items;
    let prior_iterations = node_runs
        .iter()
        .filter(|run| run.node_id == node.id && run.status == WorkflowStatus::Succeeded)
        .count() as i64;
    let max_iterations = node.max_iterations.unwrap_or(i64::MAX).max(0);
    let index = prior_iterations;
    let exhausted = index >= items.len() as i64 || index >= max_iterations;
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
        }
    } else {
        LoopOutput {
            index,
            item: Some(items[index as usize].clone()),
            has_next: true,
            count: items.len(),
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

async fn process_parallel_node<T: DatabaseImpl>(
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
    let output = ParallelOutput { branches };
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

async fn process_join_node<T: DatabaseImpl>(
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

async fn process_map_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let params = runinator_workflows::parse_map_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    let run_state = WorkflowRunState::from_state(&workflow_run.state);
    let mut frame = if run_state
        .map
        .as_ref()
        .is_some_and(|frame| frame.node_id == node.id)
    {
        // re-entry: confirm the dispatched item succeeded, then record its output.
        let existing = run_state.map.clone().unwrap_or_else(|| MapFrame {
            node_id: node.id.clone(),
            target: params.target.as_str().to_string(),
            items: Vec::new(),
            index: 0,
            outputs: Vec::new(),
            concurrency: params.concurrency.unwrap_or(1),
            item: None,
        });
        if let Some(status) = latest_status(params.target.as_str(), node_runs)
            && status != WorkflowStatus::Succeeded
        {
            transition_from_node(
                db,
                workflow_run,
                node,
                &node_run,
                status,
                None,
                Some("map_item_failed".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
        append_completed_map_item(existing, params.target.as_str(), node_runs)
    } else {
        // first visit: resolve the item list and initialize the frame.
        let context = runtime_context(workflow_run, node_runs);
        let items = runinator_workflows::resolve_value_refs(&params.items, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let items = items.as_array().cloned().unwrap_or_default();
        MapFrame {
            node_id: node.id.clone(),
            target: params.target.as_str().to_string(),
            items,
            index: 0,
            outputs: Vec::new(),
            concurrency: params.concurrency.unwrap_or(1),
            item: None,
        }
    };
    if frame.index >= frame.items.len() as i64 {
        let output = MapOutput {
            count: frame.items.len(),
            outputs: frame.outputs.clone(),
        };
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("map_exhausted".into()),
            node_runs,
        )
        .await?;
        return Ok(());
    }
    frame.item = Some(frame.items[frame.index as usize].clone());
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Running,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(frame.to_wire_value()?),
        Some("map_iteration".into()),
        None,
    )
    .await?;
    let mut state = WorkflowRunState::from_state(&workflow_run.state);
    state.map = Some(frame);
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(params.target.into_string()),
        Some(state.to_state()),
        None,
    )
    .await
}

async fn process_race_node<T: DatabaseImpl>(
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
        let output = RaceOutput { winner };
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

async fn process_try_node<T: DatabaseImpl>(
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
        )
        .await;
    }
    match phase.as_str() {
        "body" => {
            let Some(status) = latest_status(params.body.as_str(), node_runs) else {
                return Ok(());
            };
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
                    )
                    .await;
                }
                transition_from_node(
                    db,
                    workflow_run,
                    node,
                    &node_run,
                    status,
                    None,
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
                )
                .await;
            }
            transition_from_node(
                db,
                workflow_run,
                node,
                &node_run,
                status,
                None,
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
            if let Some(finally) = params.finally {
                return start_try_phase(
                    db,
                    workflow_run,
                    &node_run,
                    node,
                    finally.as_str(),
                    "finally",
                    Some(status),
                )
                .await;
            }
            transition_from_node(
                db,
                workflow_run,
                node,
                &node_run,
                status,
                None,
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
                None,
                Some("try_finally_completed".into()),
                node_runs,
            )
            .await?;
            Ok(())
        }
        _ => block_node(db, workflow_run, node, "Try node has invalid phase").await,
    }
}

async fn process_approval_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    // a loop body re-entering this node sees the prior iteration's resolved run; treat it as a
    // fresh visit so a new approval is requested instead of transitioning from the stale run.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::ApprovalRequired && timed_out(node, node_run) {
            return time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Approval timed out",
                node_runs,
            )
            .await;
        }
        if node_run.status == WorkflowStatus::Succeeded {
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                node_run.output_json.clone(),
                Some("approval_resolved".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
        return Ok(());
    }
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let params = runinator_workflows::parse_approval_parameters(node);
    let record = ApprovalRecord {
        workflow_run_id: workflow_run.id,
        node_id: node.id.clone(),
        approval_type: params.approval_type,
        prompt: params.prompt,
        status: "pending".into(),
        provider: "runinator".into(),
        resource_type: "approval_request".into(),
        external_id: format!("workflow:{}:node:{}", workflow_run.id, node.id),
        metadata: params.metadata,
    };
    let approval = db
        .create_automation_record("approval_requests".into(), record.to_wire_value()?)
        .await?;
    let approval_state = ApprovalState {
        approval: node.parameters.clone().into(),
        approval_id: approval.get("id").and_then(Value::as_i64),
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::ApprovalRequired,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(approval_state.to_wire_value()?),
        Some(WorkflowStatus::ApprovalRequired.as_str().into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::ApprovalRequired,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    arm_node_timeout(db, workflow_run.id, node).await
}

async fn process_subflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    // a loop body re-entering this node sees the prior iteration's linked subflow; treat it as a
    // fresh visit so a new child run is spawned instead of re-linking the stale one.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest
        && let Ok(subflow_state) = SubflowState::from_wire_value(&node_run.state)
    {
        let subflow_run_id = subflow_state.subflow_run_id;
        if node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
            transition_from_node(
                db,
                workflow_run,
                node,
                node_run,
                WorkflowStatus::Succeeded,
                Some(node_run.state.clone()),
                Some("subflow_linked".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
        let Some(subflow_run) = db.fetch_workflow_run(subflow_run_id).await? else {
            return Err(Box::new(RuntimeError::new(
                "workflow.subflow.run_missing".into(),
                format!("Subflow run {subflow_run_id} not found"),
            )));
        };
        match subflow_run.status {
            WorkflowStatus::Succeeded => {
                let output = SubflowOutcome {
                    subflow_run_id,
                    status: subflow_run.status.as_str().to_string(),
                    state: Some(subflow_run.state),
                    parameters: Some(subflow_run.parameters),
                };
                transition_from_node(
                    db,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("subflow_succeeded".into()),
                    node_runs,
                )
                .await?;
                return Ok(());
            }
            WorkflowStatus::Failed
            | WorkflowStatus::TimedOut
            | WorkflowStatus::Canceled
            | WorkflowStatus::Blocked => {
                let output = SubflowOutcome {
                    subflow_run_id,
                    status: subflow_run.status.as_str().to_string(),
                    state: None,
                    parameters: None,
                };
                transition_from_node(
                    db,
                    workflow_run,
                    node,
                    node_run,
                    WorkflowStatus::Failed,
                    Some(output.to_wire_value()?),
                    subflow_run
                        .message
                        .or(Some("Subflow did not succeed".into())),
                    node_runs,
                )
                .await?;
                return Ok(());
            }
            other => {
                // wait-type subflow still in flight; fail fast once it overruns the timeout.
                if timed_out_since_created(node, node_run) {
                    let timeout = node.timeout_seconds.unwrap_or_default();
                    let output = SubflowOutcome {
                        subflow_run_id,
                        status: other.as_str().to_string(),
                        state: None,
                        parameters: None,
                    };
                    transition_from_node(
                        db,
                        workflow_run,
                        node,
                        node_run,
                        WorkflowStatus::TimedOut,
                        Some(output.to_wire_value()?),
                        Some(format!(
                            "Subflow run {subflow_run_id} timed out after {timeout}s while {}",
                            other.as_str()
                        )),
                        node_runs,
                    )
                    .await?;
                    return Ok(());
                }
                return Ok(());
            }
        }
    }

    let subflow_id = resolve_subflow_id(db, node).await?;
    let context = runtime_context(workflow_run, node_runs);
    let parameters = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let run_name = resolve_optional_string(node.subflow.run_name.as_ref(), &context)?;
    let (subflow_run, reused) = if node.subflow.reuse_open_run {
        match run_name.as_deref() {
            Some(name) => match db
                .fetch_workflow_runs_by_name(name.to_string(), true)
                .await?
                .into_iter()
                .next()
            {
                Some(existing) => (existing, true),
                None => (
                    create_subflow_run(
                        db,
                        subflow_id,
                        parameters.clone(),
                        run_name.clone(),
                        workflow_run.id,
                        &node.id,
                    )
                    .await?,
                    false,
                ),
            },
            None => (
                create_subflow_run(
                    db,
                    subflow_id,
                    parameters.clone(),
                    None,
                    workflow_run.id,
                    &node.id,
                )
                .await?,
                false,
            ),
        }
    } else {
        (
            create_subflow_run(
                db,
                subflow_id,
                parameters.clone(),
                run_name.clone(),
                workflow_run.id,
                &node.id,
            )
            .await?,
            false,
        )
    };
    let node_run = db
        .create_workflow_node_run(workflow_run.id, node.id.clone(), parameters)
        .await?;
    let state = SubflowState {
        subflow_run_id: subflow_run.id,
        subflow_workflow_id: subflow_run.workflow_id,
        run_name,
        reused,
    }
    .to_wire_value()?;
    if node.subflow.subflow_type == WorkflowSubflowType::FireAndForget {
        db.update_workflow_node_run(
            node_run.id,
            WorkflowStatus::Succeeded,
            Some(node_run.attempt + 1),
            None,
            Some(state.clone()),
            Some(state.clone()),
            Some(if reused {
                "subflow_reused".into()
            } else {
                "subflow_started".into()
            }),
            None,
        )
        .await?;
        transition_from_node(
            db,
            workflow_run,
            node,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(state.clone()),
            Some("subflow_linked".into()),
            node_runs,
        )
        .await?;
        return Ok(());
    }

    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.clone()),
        Some("subflow_started".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        Some(state),
        None,
    )
    .await?;
    arm_node_timeout(db, workflow_run.id, node).await
}

/// resolve a subflow node's target workflow id from an explicit id or workflow name.
async fn resolve_subflow_id<T: DatabaseImpl>(
    db: &T,
    node: &WorkflowNode,
) -> Result<i64, SendableError> {
    if let Some(subflow_id) = node.subflow_id {
        return Ok(subflow_id);
    }
    if let Some(workflow_name) = node.subflow.workflow_name.as_deref() {
        let workflow_name = workflow_name.trim();
        if !workflow_name.is_empty() {
            let workflow = db
                .fetch_workflow_by_name(workflow_name.to_string())
                .await?
                .ok_or_else(|| {
                    Box::new(RuntimeError::new(
                        "workflow.subflow.missing".into(),
                        format!("Subflow workflow {workflow_name} not found"),
                    )) as SendableError
                })?;
            if let Some(id) = workflow.id {
                return Ok(id);
            }
            return Err(Box::new(RuntimeError::new(
                "workflow.subflow.missing_id".into(),
                format!("Subflow workflow {workflow_name} has no id"),
            )));
        }
    }
    Err(Box::new(RuntimeError::new(
        "workflow.subflow.target_missing".into(),
        format!("Subflow node {} is missing a target", node.id),
    )))
}

/// create a child workflow run, stamp its parent linkage into state, and enqueue its start node so
/// the reducer drives it. the parent linkage lets a terminal child wake the waiting parent node.
async fn create_subflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
    parameters: Value,
    run_name: Option<String>,
    parent_run_id: i64,
    parent_node_id: &str,
) -> Result<WorkflowRun, SendableError> {
    let snapshot = db.fetch_workflow(workflow_id).await?.ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow.not_found".into(),
            format!("Workflow {workflow_id} not found"),
        )) as SendableError
    })?;
    let state = runinator_models::json!({
        "control": { "pause_requested": false },
        "subflow_parent": { "run_id": parent_run_id, "node_id": parent_node_id }
    });
    let run = db
        .create_workflow_run(workflow_id, snapshot, parameters, state, run_name)
        .await?;
    if let Some(snapshot) = run.workflow_snapshot.as_ref() {
        let (start, _) = runinator_workflows::parse_nodes(snapshot)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let event = NewOrchestrationEvent::new(
            run.id,
            Some(start.clone()),
            "subflow_run_created",
            runinator_models::json!({ "workflow_id": run.workflow_id, "node_id": start }),
        );
        db.enqueue_ready_node(event, start, Utc::now()).await?;
    }
    Ok(run)
}

fn resolve_optional_string(
    value: Option<&Value>,
    context: &Value,
) -> Result<Option<String>, SendableError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let resolved = runinator_workflows::resolve_value_refs(value, context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let name = match resolved {
        Value::Null => None,
        Value::String(value) => Some(value.trim().to_string()).filter(|value| !value.is_empty()),
        other => Some(other.to_string()),
    };
    Ok(name)
}

// --- shared db-direct reducer helpers -----------------------------------------

/// settle a node run, retrying while attempts remain, otherwise transitioning.
#[allow(clippy::too_many_arguments)]
async fn retry_or_transition<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    if node_run.attempt < node.retry.max_attempts {
        db.update_workflow_node_run(
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
        db.update_workflow_run_status(
            workflow_run.id,
            WorkflowStatus::Running,
            Some(node.id.clone()),
            None,
            None,
        )
        .await
    } else {
        transition_from_node(
            db,
            workflow_run,
            node,
            node_run,
            status,
            output_json,
            message,
            node_runs,
        )
        .await?;
        Ok(())
    }
}

/// time out the in-flight run with a node-specific message, retrying if attempts remain.
async fn time_out<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    message: &str,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    retry_or_transition(
        db,
        workflow_run,
        node,
        node_run,
        WorkflowStatus::TimedOut,
        None,
        Some(message.into()),
        node_runs,
    )
    .await
}

/// create a node run and block the workflow with a message.
async fn block_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    message: &str,
) -> Result<(), SendableError> {
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    db.update_workflow_node_run(
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
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Blocked,
        Some(node.id.clone()),
        None,
        Some(message.into()),
    )
    .await
}

/// advance a try node into a phase (body/catch/finally), recording the phase frame.
async fn start_try_phase<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node_run: &WorkflowNodeRun,
    node: &WorkflowNode,
    target: &str,
    phase: &str,
    pending_status: Option<WorkflowStatus>,
) -> Result<(), SendableError> {
    let frame = TryFrame {
        node_id: node.id.clone(),
        phase: phase.into(),
        pending_status,
    };
    let mut run_state = WorkflowRunState::from_state(&workflow_run.state);
    run_state.try_frame = Some(frame.clone());
    let state = run_state.to_state();
    db.update_workflow_node_run(
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
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Running,
        Some(target.into()),
        Some(state),
        None,
    )
    .await
}

/// true when the run started more than `node.timeout_seconds` ago.
fn timed_out(node: &WorkflowNode, run: &WorkflowNodeRun) -> bool {
    let (Some(timeout), Some(started_at)) = (node.timeout_seconds, run.started_at) else {
        return false;
    };
    Utc::now() - started_at > chrono::Duration::seconds(timeout)
}

/// like `timed_out`, but measured from run creation (used by subflow waits).
fn timed_out_since_created(node: &WorkflowNode, run: &WorkflowNodeRun) -> bool {
    let Some(timeout) = node.timeout_seconds else {
        return false;
    };
    Utc::now() - run.created_at > chrono::Duration::seconds(timeout)
}

/// enqueue a delayed self ready node at a node's timeout deadline. the event-driven ready queue does
/// not re-poll parked nodes, so a node that parks (approval/join/subflow) re-arms its own timeout so
/// the timeout check fires even when no external wake-up arrives.
async fn arm_node_timeout<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    node: &WorkflowNode,
) -> Result<(), SendableError> {
    let Some(timeout) = node.timeout_seconds else {
        return Ok(());
    };
    let deadline = Utc::now() + chrono::Duration::seconds(timeout);
    let event = NewOrchestrationEvent::new(
        workflow_run_id,
        Some(node.id.clone()),
        "node_timeout_rearm",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), deadline)
        .await?;
    Ok(())
}

/// when a child workflow run reaches a terminal state, wake the parent subflow node waiting on it.
/// the parent linkage is stamped into the child run's `state.subflow_parent` at creation.
async fn maybe_wake_subflow_parent<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Result<(), SendableError> {
    if !run.status.is_terminal() {
        return Ok(());
    }
    let Some(parent) = run.state.get("subflow_parent") else {
        return Ok(());
    };
    let (Some(parent_run_id), Some(parent_node_id)) = (
        parent.get("run_id").and_then(Value::as_i64),
        parent.get("node_id").and_then(Value::as_str),
    ) else {
        return Ok(());
    };
    let event = NewOrchestrationEvent::new(
        parent_run_id,
        Some(parent_node_id.to_string()),
        "subflow_child_finished",
        runinator_models::json!({ "child_run_id": run.id, "status": run.status.as_str() }),
    );
    db.enqueue_ready_node(event, parent_node_id.to_string(), Utc::now())
        .await?;
    Ok(())
}

async fn transition_from_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_run: &WorkflowNodeRun,
    status: WorkflowStatus,
    output_json: Option<Value>,
    message: Option<String>,
    node_runs: &[WorkflowNodeRun],
) -> Result<Option<String>, SendableError> {
    db.update_workflow_node_run(
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
    let mut context = runtime_context(workflow_run, node_runs);
    if let Some(output) = output_json {
        set_step_output(&mut context, &node.id, output);
    }
    let next = runinator_workflows::next_transition(node, status, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    match next {
        Some(next) => {
            db.update_workflow_run_status(
                workflow_run.id,
                WorkflowStatus::Running,
                Some(next.clone()),
                None,
                message,
            )
            .await?;
            Ok(Some(next))
        }
        None if status == WorkflowStatus::Succeeded => {
            db.update_workflow_run_status(
                workflow_run.id,
                WorkflowStatus::Succeeded,
                Some(node.id.clone()),
                None,
                message,
            )
            .await?;
            Ok(None)
        }
        None => {
            db.update_workflow_run_status(
                workflow_run.id,
                status,
                Some(node.id.clone()),
                None,
                message,
            )
            .await?;
            Ok(None)
        }
    }
}

async fn ensure_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Result<WorkflowNodeRun, SendableError> {
    if let Some(latest) = latest {
        return Ok(latest.clone());
    }
    db.create_workflow_node_run(
        workflow_run.id,
        node.id.clone(),
        node.parameters.clone().into(),
    )
    .await
}

async fn ensure_completed_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    reason: &str,
) -> Result<(), SendableError> {
    if latest.is_some_and(|run| run.status == WorkflowStatus::Succeeded) {
        return Ok(());
    }
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    db.update_workflow_node_run(
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

fn build_node_parameters(
    action: &WorkflowAction,
    node: &WorkflowNode,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
) -> Result<Value, SendableError> {
    let base = merge_parameters(&action.configuration, &node.parameters);
    let context = runtime_context(workflow_run, node_runs);
    runinator_workflows::resolve_value_refs(&base, &context)
        .map_err(|err| -> SendableError { Box::new(err) })
}

fn build_action_command(
    workflow_run_id: i64,
    node_run: &WorkflowNodeRun,
    action: &WorkflowAction,
    parameters: Value,
) -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id: node_run.id,
        node_id: node_run.node_id.clone(),
        action: action.clone(),
        attempt: node_run.attempt + 1,
        parameters,
    }
}

fn runtime_context(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> Value {
    let prev_output = node_runs
        .iter()
        .filter_map(|run| run.output_json.clone())
        .next_back();
    let outputs = node_runs
        .iter()
        .filter_map(|run| {
            run.output_json
                .clone()
                .map(|output| (run.node_id.clone(), output))
        })
        .collect::<HashMap<_, _>>();
    let mut context = runinator_workflows::outputs_context(&workflow_run.parameters, &outputs);
    if let Some(object) = context.as_object_mut() {
        let header = WorkflowContextHeader {
            run_id: workflow_run.id,
            workflow_id: workflow_run.workflow_id,
            state: workflow_run.state.clone(),
        };
        object.insert(
            "workflow".into(),
            header.to_wire_value().unwrap_or(Value::Null),
        );
        if let Some(prev) = prev_output {
            object.insert("prev".into(), prev);
        }
        // config refs (`{"$ref":{"config":[...]}}`) resolve here, before any action command
        // is published; secrets stay unresolved until the worker.
        object.insert("config".into(), crate::handlers::credentials::config_tree());
    }
    context
}

fn set_step_output(scope: &mut Value, node_id: &str, output: Value) {
    if let Some(slot) = scope.pointer_mut(&format!("/steps/{node_id}/output")) {
        *slot = output;
    }
}

fn merge_parameters(defaults: &Value, parameters: &Value) -> Value {
    match (defaults, parameters) {
        (Value::Object(defaults), Value::Object(parameters)) => {
            let mut merged = defaults.clone();
            for (key, value) in parameters {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        (_, Value::Null) => defaults.clone(),
        _ => parameters.clone(),
    }
}

fn latest_node_run<'a>(
    node_runs: &'a [WorkflowNodeRun],
    node_id: &str,
) -> Option<&'a WorkflowNodeRun> {
    node_runs
        .iter()
        .filter(|run| run.node_id == node_id)
        .max_by_key(|run| run.id)
}

// true when a resumable node is re-entered with a terminal run from a prior visit. a loop body (or
// any back-edge) drives control past the node and returns to it, leaving the previous iteration's
// run as `latest`; the intervening control node always records a newer node run, so a node run
// created after `latest` means control already left and came back. such a node must start a fresh
// visit instead of resuming or transitioning from the stale run, otherwise the body only runs once.
fn is_reentry_stale(latest: &WorkflowNodeRun, node_runs: &[WorkflowNodeRun]) -> bool {
    latest.status.is_terminal() && node_runs.iter().any(|run| run.id > latest.id)
}

#[derive(Debug, PartialEq, Eq)]
struct WorkflowProgressKey {
    status: WorkflowStatus,
    active_node_id: Option<String>,
    node_count: usize,
    latest_active_node_run_id: Option<i64>,
    latest_active_node_status: Option<WorkflowStatus>,
}

impl WorkflowProgressKey {
    async fn from_run<T: DatabaseImpl>(
        db: &T,
        workflow_run_id: i64,
    ) -> Result<Self, SendableError> {
        let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
            return Err(Box::new(RuntimeError::new(
                "workflow_run.not_found".into(),
                format!("Workflow run {workflow_run_id} not found"),
            )));
        };
        let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
        Ok(Self::from_parts(&run, &nodes))
    }

    fn from_parts(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> Self {
        let latest_active = workflow_run
            .active_node_id
            .as_deref()
            .and_then(|active| latest_node_run(node_runs, active));
        Self {
            status: workflow_run.status,
            active_node_id: workflow_run.active_node_id.clone(),
            node_count: node_runs.len(),
            latest_active_node_run_id: latest_active.map(|run| run.id),
            latest_active_node_status: latest_active.map(|run| run.status),
        }
    }
}

fn should_stop_inline_progress(
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
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
    let Some(active_node_id) = workflow_run.active_node_id.as_deref() else {
        return false;
    };
    latest_node_run(node_runs, active_node_id).is_some_and(|run| {
        matches!(
            run.status,
            WorkflowStatus::Running | WorkflowStatus::Waiting | WorkflowStatus::ApprovalRequired
        )
    })
}

/// true when the run's active node is an action node, the one node kind that parks the run `Running`
/// awaiting a worker result that will not arrive inline. control nodes re-enter inline instead.
async fn active_node_awaits_worker<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
) -> Result<bool, SendableError> {
    let Some(active_node_id) = run.active_node_id.as_deref() else {
        return Ok(false);
    };
    let workflow = match run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => match db.fetch_workflow(run.workflow_id).await? {
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
        .is_some_and(|node| node.kind == WorkflowNodeKind::Action))
}
