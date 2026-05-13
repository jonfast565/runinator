use runinator_broker::Broker;
use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;
use std::collections::HashMap;

use crate::{api::WorkflowSchedulerApi, context::latest_node_run, nodes::*};

pub async fn run_workflow_iteration(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
) -> Result<(), SendableError> {
    for status in [
        WorkflowStatus::Queued,
        WorkflowStatus::Running,
        WorkflowStatus::Waiting,
        WorkflowStatus::ApprovalRequired,
        WorkflowStatus::Blocked,
    ] {
        for run in api.fetch_workflow_runs_by_status(status).await? {
            process_workflow_run(broker, api, run).await?;
        }
    }
    Ok(())
}

pub async fn process_workflow_run(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
    workflow_run: WorkflowRun,
) -> Result<(), SendableError> {
    let workflow = api.fetch_workflow(workflow_run.workflow_id).await?;
    let (start, nodes) = runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let (_, node_runs) = api.fetch_workflow_run(workflow_run.id).await?;
    let active_node_id = workflow_run
        .active_node_id
        .clone()
        .unwrap_or_else(|| start.clone());
    let node_by_id = nodes
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect::<HashMap<_, _>>();
    let Some(node) = node_by_id.get(&active_node_id) else {
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Failed,
            Some(active_node_id),
            None,
            Some("Active workflow node is missing".into()),
        )
        .await?;
        return Ok(());
    };
    let latest = latest_node_run(&node_runs, &active_node_id);
    if let Some(decision) = reentry_exhaustion(node, latest, &node_runs) {
        match decision {
            ReentryExhaustion::Route(target) => {
                api.update_workflow_run(
                    workflow_run.id,
                    WorkflowStatus::Running,
                    Some(target),
                    None,
                    Some("Reentry visit limit exhausted".into()),
                )
                .await?;
            }
            ReentryExhaustion::Block => {
                api.update_workflow_run(
                    workflow_run.id,
                    WorkflowStatus::Blocked,
                    Some(active_node_id),
                    None,
                    Some("Reentry visit limit exhausted".into()),
                )
                .await?;
            }
        }
        return Ok(());
    }
    match node.kind {
        WorkflowNodeKind::Start => {
            process_start_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Task => {
            process_task_node(broker, api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Wait => {
            process_wait_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Condition => {
            process_condition_node(api, &workflow_run, node, &node_runs).await?
        }
        WorkflowNodeKind::Switch => {
            process_switch_node(api, &workflow_run, node, &node_runs).await?
        }
        WorkflowNodeKind::Approval => {
            process_approval_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Loop => {
            process_loop_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Parallel => {
            process_parallel_node(api, &workflow_run, node, latest).await?
        }
        WorkflowNodeKind::Join => {
            process_join_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Try => {
            process_try_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Map => {
            process_map_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Race => {
            process_race_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Emit => process_emit_node(api, &workflow_run, node, &node_runs).await?,
        WorkflowNodeKind::Subflow => {
            process_subflow_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::End => {
            ensure_completed_node_run(api, &workflow_run, node, latest, "end_reached").await?;
            if let Some(loop_node) = workflow_run
                .state
                .pointer("/loop/return_to")
                .and_then(Value::as_str)
            {
                api.update_workflow_run(
                    workflow_run.id,
                    WorkflowStatus::Running,
                    Some(loop_node.to_string()),
                    Some(serde_json::json!({ "loop": {} })),
                    None,
                )
                .await?;
                return Ok(());
            }
            api.update_workflow_run(
                workflow_run.id,
                WorkflowStatus::Succeeded,
                Some(node.id.clone()),
                None,
                None,
            )
            .await?;
        }
    };

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReentryExhaustion {
    Route(String),
    Block,
}

pub(crate) fn reentry_exhaustion(
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Option<ReentryExhaustion> {
    if !node.reentry.enabled {
        return None;
    }
    if latest.is_some_and(|run| run.status.is_active()) {
        return None;
    }
    let visits = node_runs
        .iter()
        .filter(|run| run.node_id == node.id)
        .count() as i64;
    if visits < node.reentry.max_visits {
        return None;
    }
    Some(
        node.reentry
            .on_exhausted
            .as_ref()
            .map(|target| ReentryExhaustion::Route(target.as_str().to_string()))
            .unwrap_or(ReentryExhaustion::Block),
    )
}

async fn process_start_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let created;
    let node_run = if let Some(latest) = latest {
        latest
    } else {
        created = api
            .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
            .await?;
        &created
    };
    transition_from_node(
        api,
        workflow_run,
        node,
        node_run,
        WorkflowStatus::Succeeded,
        None,
        Some("start_reached".into()),
        node_runs,
    )
    .await
}

async fn ensure_completed_node_run(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    reason: &str,
) -> Result<(), SendableError> {
    if latest.is_some_and(|run| run.status == WorkflowStatus::Succeeded) {
        return Ok(());
    }
    let created;
    let node_run = if let Some(latest) = latest {
        latest
    } else {
        created = api
            .create_workflow_node_run(workflow_run.id, &node.id, node.parameters.clone())
            .await?;
        &created
    };
    api.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        None,
        Some(node_run.attempt + 1),
        None,
        None,
        None,
        Some(reason.into()),
        None,
    )
    .await
}
