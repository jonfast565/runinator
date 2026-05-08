use std::collections::HashMap;
use runinator_broker::Broker;
use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowNodeKind, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;

use crate::{api::SchedulerApi, nodes::*, context::latest_node_run};

pub async fn run_workflow_iteration(
    broker: &dyn Broker,
    api: &SchedulerApi,
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
    api: &SchedulerApi,
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
    match node.kind {
        WorkflowNodeKind::Task => {
            process_task_node(broker, api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Wait => {
            process_wait_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Condition => {
            process_condition_node(api, &workflow_run, node, &node_runs).await?
        }
        WorkflowNodeKind::Approval => {
            process_approval_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Loop => {
            process_loop_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::Subflow => {
            process_subflow_node(api, &workflow_run, node, latest, &node_runs).await?
        }
        WorkflowNodeKind::End => {
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
