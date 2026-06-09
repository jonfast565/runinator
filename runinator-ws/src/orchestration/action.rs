use super::context::{is_reentry_stale, merge_parameters, runtime_context};
use super::transitions::retry_or_transition;
use super::*;
use uuid::Uuid;

pub(super) async fn process_action_node<T: DatabaseImpl>(
    db: &T,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let action = node
        .action
        .as_ref()
        .ok_or_else(|| crate::errors::ACTION_CONFIG_MISSING.error(&node.id))?;
    // a loop body re-entering this node sees the prior iteration's terminal run; treat it as a
    // fresh visit so the action dispatches again instead of transitioning from the stale run.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::Running {
            return Ok(());
        }
        if node_run.status.is_terminal() {
            retry_or_transition(
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
    let parameters =
        build_node_parameters(db, workflow, action, node, workflow_run, node_runs).await?;
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

async fn build_node_parameters<T: DatabaseImpl>(
    db: &T,
    workflow: &runinator_models::workflows::WorkflowDefinition,
    action: &WorkflowAction,
    node: &WorkflowNode,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
) -> Result<Value, SendableError> {
    // an effectful `std.exec` program is interpreted by the worker, not resolved here: ship the
    // program verbatim alongside the full runtime context and the workflow's user-function table so
    // the worker's interpreter can resolve refs/calls (with the effectful library) against it.
    if action.provider == "std" {
        let context = runtime_context(db, workflow_run, node_runs).await;
        let program = action
            .configuration
            .as_value()
            .get("program")
            .cloned()
            .unwrap_or(Value::Null);
        let functions = workflow
            .definition
            .metadata
            .get("functions")
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(
            runinator_models::json!({ "program": program, "context": context, "functions": functions }),
        );
    }
    let base = merge_parameters(&action.configuration, &node.parameters);
    let context = runtime_context(db, workflow_run, node_runs).await;
    runinator_workflows::resolve_value_refs(&base, &context)
        .map_err(|err| -> SendableError { Box::new(err) })
}

fn build_action_command(
    workflow_run_id: Uuid,
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
