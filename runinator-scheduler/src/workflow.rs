use chrono::{Duration, Utc};
use log::warn;
use runinator_broker::Broker;
use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;
use std::collections::HashMap;

use crate::{
    api::WorkflowSchedulerApi,
    config::Config,
    context::{build_node_parameters, latest_node_run, runtime_context},
    control, debug,
    nodes::*,
};

const MAX_INLINE_WORKFLOW_STEPS: usize = 64;

pub async fn run_workflow_iteration(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
    config: &Config,
) -> Result<(), SendableError> {
    let statuses = [
        WorkflowStatus::Queued,
        WorkflowStatus::Running,
        WorkflowStatus::DebugPaused,
        WorkflowStatus::Waiting,
        WorkflowStatus::ApprovalRequired,
        WorkflowStatus::Blocked,
    ];
    let lease_until = Utc::now() + Duration::seconds(config.scheduler_lease_seconds as i64);
    let runs = api
        .claim_workflow_runs_for_scheduler(
            &config.scheduler_id,
            &statuses,
            lease_until,
            config.scheduler_claim_limit,
        )
        .await?;
    for run in runs {
        let renewed = api
            .renew_workflow_run_claim(
                run.id,
                &config.scheduler_id,
                Utc::now() + Duration::seconds(config.scheduler_lease_seconds as i64),
            )
            .await?;
        if renewed {
            process_workflow_run(broker, api, run.clone()).await?;
        } else {
            warn!(
                "Scheduler {} lost workflow run claim {}; skipping",
                config.scheduler_id, run.id
            );
        }
        if let Err(err) = api
            .release_workflow_run_claim(run.id, &config.scheduler_id)
            .await
        {
            warn!(
                "Scheduler {} failed releasing workflow run claim {}: {}",
                config.scheduler_id, run.id, err
            );
        }
    }
    Ok(())
}

pub async fn process_workflow_run(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
    mut workflow_run: WorkflowRun,
) -> Result<(), SendableError> {
    if workflow_run.status == WorkflowStatus::Queued {
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Running,
            workflow_run.active_node_id.clone(),
            None,
            Some("Workflow run claimed by scheduler".into()),
        )
        .await?;
        workflow_run.status = WorkflowStatus::Running;
    }

    if should_pause_without_debug(api, &workflow_run).await? {
        return Ok(());
    }

    for _ in 0..MAX_INLINE_WORKFLOW_STEPS {
        let before = WorkflowProgressKey::from_run(api, &workflow_run).await?;
        process_workflow_run_step(broker, api, workflow_run.clone()).await?;
        let (next_run, next_nodes) = api.fetch_workflow_run(workflow_run.id).await?;
        let after = WorkflowProgressKey::from_parts(&next_run, &next_nodes);
        if should_stop_inline_progress(&next_run, &next_nodes) || before == after {
            return Ok(());
        }
        workflow_run = next_run;
    }

    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Blocked,
        workflow_run.active_node_id.clone(),
        None,
        Some("Inline workflow progress limit exhausted".into()),
    )
    .await?;
    Ok(())
}

pub async fn process_workflow_run_step(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
    workflow_run: WorkflowRun,
) -> Result<(), SendableError> {
    if workflow_run.status.is_terminal() || workflow_run.status == WorkflowStatus::Paused {
        return Ok(());
    }
    let workflow = match workflow_run.workflow_snapshot.clone() {
        Some(snapshot) => snapshot,
        None => api.fetch_workflow(workflow_run.workflow_id).await?,
    };
    let providers = api.fetch_providers().await?;
    let (start, nodes) =
        runinator_workflows::validate_workflow_with_providers(&workflow, &providers)
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
        } else if debug::step_requested(&workflow_run) || debug::paused(&workflow_run) {
            let mut next = workflow_run.clone();
            next.status = WorkflowStatus::Running;
            next.state = debug::state_with_step_cleared(workflow_run.state.clone())?;
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
    if node.skipped {
        process_skipped_node(api, &workflow_run, node, latest, &node_runs).await?;
        return Ok(());
    }
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
    dispatch_node(broker, api, &workflow_run, node, latest, &node_runs).await?;

    Ok(())
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
    async fn from_run(
        api: &dyn WorkflowSchedulerApi,
        workflow_run: &WorkflowRun,
    ) -> Result<Self, SendableError> {
        let (run, nodes) = api.fetch_workflow_run(workflow_run.id).await?;
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

fn should_stop_inline_progress(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> bool {
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

async fn should_pause_for_debug(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<bool, SendableError> {
    if !debug::enabled(workflow_run) || debug::step_requested(workflow_run) {
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
    if workflow_run.status == WorkflowStatus::DebugPaused && debug::paused(workflow_run) {
        return Ok(true);
    }
    if control::pause_requested(workflow_run) {
        let state = debug_pause_state(api, &workflow_run, node, node_runs).await?;
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::DebugPaused,
            Some(node.id.clone()),
            Some(state),
            Some(format!("Debug paused before node {}", node.id)),
        )
        .await?;
        crate::nodes::fire_paused(api, workflow_run, node, latest, node_runs).await;
        return Ok(true);
    }
    if !debug::should_break_at(workflow_run, &node.id) {
        return Ok(false);
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
    crate::nodes::fire_paused(api, workflow_run, node, latest, node_runs).await;
    Ok(true)
}

async fn should_pause_without_debug(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
) -> Result<bool, SendableError> {
    if !control::pause_requested(workflow_run) || debug::enabled(workflow_run) {
        return Ok(false);
    }
    let Some(active_node_id) = workflow_run.active_node_id.as_deref() else {
        api.update_workflow_run(
            workflow_run.id,
            WorkflowStatus::Paused,
            workflow_run.active_node_id.clone(),
            None,
            Some("Workflow pause requested".into()),
        )
        .await?;
        return Ok(true);
    };
    let (_, node_runs) = api.fetch_workflow_run(workflow_run.id).await?;
    let latest = latest_node_run(&node_runs, active_node_id);
    if latest.is_some_and(|run| run.status == WorkflowStatus::Running) {
        return Ok(false);
    }
    api.update_workflow_run(
        workflow_run.id,
        WorkflowStatus::Paused,
        workflow_run.active_node_id.clone(),
        None,
        Some("Workflow pause requested".into()),
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
    let input = debug_input_json(api, workflow_run, node, node_runs).await?;
    let context = runtime_context(workflow_run, node_runs);
    let last_output = node_runs
        .iter()
        .filter_map(|run| run.output_json.clone())
        .last()
        .unwrap_or(Value::Null);

    let one_shot_consumed = matches!(
        debug::one_shot_breakpoint(workflow_run),
        Some(ref id) if id == &node.id
    );

    let mut run_state = RunState::from_run(workflow_run);
    run_state.ensure_control();
    // preserve user-owned fields (mode, breakpoints) while updating the runtime debug fields.
    let debug = run_state.debug_mut();
    debug.enabled = true;
    debug.paused = true;
    debug.step_requested = false;
    debug.current_node_id = Some(node.id.clone());
    debug.current_node_kind = Some(node.kind.clone());
    debug.input_json = Some(input);
    debug.context_json = Some(context);
    debug.last_output_json = Some(last_output);
    if one_shot_consumed {
        debug.one_shot_breakpoint = None;
    }
    Ok(run_state.into_value()?)
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
