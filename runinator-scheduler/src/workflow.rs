use runinator_broker::Broker;
use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;
use std::collections::HashMap;

use crate::{
    api::WorkflowSchedulerApi,
    context::{build_node_parameters, latest_node_run, runtime_context},
    nodes::*,
};

pub async fn run_workflow_iteration(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
) -> Result<(), SendableError> {
    for status in [
        WorkflowStatus::Queued,
        WorkflowStatus::Running,
        WorkflowStatus::DebugPaused,
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
    let workflow_run =
        if should_pause_for_debug(api, &workflow_run, node, latest, &node_runs).await? {
            return Ok(());
        } else if debug_step_requested(&workflow_run) {
            let mut next = workflow_run.clone();
            next.status = WorkflowStatus::Running;
            next.state = debug_state_with_step_cleared(workflow_run.state.clone());
            api.update_workflow_run(
                next.id,
                WorkflowStatus::Running,
                Some(active_node_id.clone()),
                Some(next.state.clone()),
                None,
            )
            .await?;
            next
        } else {
            workflow_run
        };
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
        WorkflowNodeKind::Action => {
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

async fn should_pause_for_debug(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<bool, SendableError> {
    if !debug_enabled(workflow_run) || debug_step_requested(workflow_run) {
        return Ok(false);
    }
    if workflow_run.status.is_terminal() {
        return Ok(false);
    }
    if latest.is_some_and(|run| {
        matches!(
            run.status,
            WorkflowStatus::Running | WorkflowStatus::Waiting | WorkflowStatus::ApprovalRequired
        )
    }) {
        return Ok(false);
    }
    if workflow_run.status == WorkflowStatus::DebugPaused && debug_paused(workflow_run) {
        return Ok(true);
    }

    let state = debug_pause_state(api, &workflow_run, node, node_runs).await?;
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::DebugPaused,
        Some(node.id.clone()),
        Some(state),
        Some(format!("Debug paused before node {}", node.id)),
    )
    .await?;
    Ok(true)
}

async fn debug_pause_state(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<Value, SendableError> {
    let mut state = workflow_run.state.clone();
    if !state.is_object() {
        state = serde_json::json!({});
    }
    let input = debug_input_json(api, workflow_run, node, node_runs).await?;
    let context = runtime_context(workflow_run, node_runs);
    let last_output = node_runs
        .iter()
        .filter_map(|run| run.output_json.clone())
        .last()
        .unwrap_or(Value::Null);

    let Some(object) = state.as_object_mut() else {
        return Ok(state);
    };
    object.insert(
        "debug".into(),
        serde_json::json!({
            "enabled": true,
            "paused": true,
            "step_requested": false,
            "current_node_id": node.id,
            "current_node_kind": node.kind,
            "input_json": input,
            "context_json": context,
            "last_output_json": last_output
        }),
    );
    Ok(state)
}

async fn debug_input_json(
    _api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    node_runs: &[WorkflowNodeRun],
) -> Result<Value, SendableError> {
    if node.kind == WorkflowNodeKind::Action {
        if let Some(action) = &node.action {
            return build_node_parameters(action, node, workflow_run, node_runs);
        }
    }
    let context = runtime_context(workflow_run, node_runs);
    runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })
}

fn debug_enabled(workflow_run: &WorkflowRun) -> bool {
    workflow_run
        .state
        .pointer("/debug/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn debug_paused(workflow_run: &WorkflowRun) -> bool {
    workflow_run
        .state
        .pointer("/debug/paused")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn debug_step_requested(workflow_run: &WorkflowRun) -> bool {
    workflow_run
        .state
        .pointer("/debug/step_requested")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn debug_state_with_step_cleared(mut state: Value) -> Value {
    if let Some(debug) = state.get_mut("debug").and_then(Value::as_object_mut) {
        debug.insert("paused".into(), Value::Bool(false));
        debug.insert("step_requested".into(), Value::Bool(false));
    }
    state
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
        Some(node_run.attempt + 1),
        None,
        None,
        None,
        Some(reason.into()),
        None,
    )
    .await
}
