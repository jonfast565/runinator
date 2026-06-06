use super::context::runtime_context;
use super::*;
use super::{action, approval, basic, context, control_flow, subflow, transitions, wait};

const MAX_INLINE_WORKFLOW_STEPS: usize = 64;

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
            transitions::maybe_wake_subflow_parent(db, &next_run).await?;
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
            .ok_or_else(|| crate::errors::WORKFLOW_NOT_FOUND.error(workflow_run.workflow_id))?,
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
    let latest = context::latest_node_run(&node_runs, &active_node_id).cloned();
    if node.skipped {
        basic::process_skipped_node(db, &workflow_run, node, latest.as_ref(), &node_runs).await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    match &node.kind {
        runinator_models::workflows::WorkflowNodeKind::Start => {
            let node_run =
                transitions::ensure_node_run(db, &workflow_run, node, latest.as_ref()).await?;
            transitions::transition_from_node(
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
            action::process_action_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                .await?;
            return Ok(ReadyNodeDisposition::Complete);
        }
        runinator_models::workflows::WorkflowNodeKind::Wait => {
            return wait::process_wait_node(db, &workflow_run, node, latest.as_ref()).await;
        }
        runinator_models::workflows::WorkflowNodeKind::Condition => {
            let node_run = db
                .create_workflow_node_run(
                    workflow_run.id,
                    node.id.clone(),
                    node.parameters.clone().into(),
                )
                .await?;
            let context = runtime_context(db, &workflow_run, &node_runs).await;
            let matched = runinator_workflows::evaluate_condition(&node.condition, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let (status, reason) = if matched {
                (WorkflowStatus::Succeeded, "condition_matched")
            } else {
                (WorkflowStatus::Blocked, "condition_unmatched")
            };
            transitions::transition_from_node(
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
            let context = runtime_context(db, &workflow_run, &node_runs).await;
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
                    transitions::transition_from_node(
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
            let context = runtime_context(db, &workflow_run, &node_runs).await;
            let data = runinator_workflows::resolve_value_refs(&params.data, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            let output = EmitOutput {
                event_type: params.event_type,
                data,
            };
            transitions::transition_from_node(
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
            basic::process_config_node(db, &workflow_run, node, &node_runs).await?;
        }
        runinator_models::workflows::WorkflowNodeKind::End => {
            transitions::ensure_completed_node_run(
                db,
                &workflow_run,
                node,
                latest.as_ref(),
                "end_reached",
            )
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
            transitions::ensure_completed_node_run(
                db,
                &workflow_run,
                node,
                latest.as_ref(),
                "fail_reached",
            )
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
            control_flow::process_loop_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Parallel => {
            control_flow::process_parallel_node(
                db,
                &workflow_run,
                node,
                latest.as_ref(),
                &node_runs,
            )
            .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Join => {
            control_flow::process_join_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Map => {
            control_flow::process_map_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Race => {
            control_flow::process_race_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Try => {
            control_flow::process_try_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Approval => {
            approval::process_approval_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                .await?;
        }
        runinator_models::workflows::WorkflowNodeKind::Subflow => {
            if let Err(err) =
                subflow::process_subflow_node(db, &workflow_run, node, latest.as_ref(), &node_runs)
                    .await
            {
                // a subflow error would otherwise bubble up and retry forever while the run stays
                // non-terminal. surface it as a failed node so the workflow can follow on_failure.
                let node_run =
                    transitions::ensure_node_run(db, &workflow_run, node, latest.as_ref()).await?;
                transitions::transition_from_node(
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

#[derive(Debug, PartialEq, Eq)]
pub(super) struct WorkflowProgressKey {
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
            return Err(crate::errors::WORKFLOW_RUN_NOT_FOUND.error(workflow_run_id));
        };
        let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
        Ok(Self::from_parts(&run, &nodes))
    }

    fn from_parts(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> Self {
        let latest_active = workflow_run
            .active_node_id
            .as_deref()
            .and_then(|active| context::latest_node_run(node_runs, active));
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
    context::latest_node_run(node_runs, active_node_id).is_some_and(|run| {
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
