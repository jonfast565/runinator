use super::context::runtime_context;
use super::transitions::timed_out;
use super::transitions::transition_from_node;
use super::*;

pub(super) async fn process_input_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let latest = latest.filter(|run| run.node_id == node.id);
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::InputRequired && timed_out(node, node_run) {
            return transitions::time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Input timed out",
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
                Some("input_resolved".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
        if node_run.status == WorkflowStatus::InputRequired {
            return Ok(());
        }
    }

    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let state = InputState {
        input: node.parameters.clone().into(),
        input_id: None,
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::InputRequired,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.to_wire_value()?),
        Some(WorkflowStatus::InputRequired.as_str().into()),
        Some("input_requested".into()),
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::InputRequired,
        Some(node.id.clone()),
        Some(state.to_wire_value()?),
        Some("input_requested".into()),
    )
    .await?;
    Ok(())
}
