use super::transitions::transition_from_node;
use super::*;

const RECORD_TYPE: &str = "workflow_checkpoint";

/// parse the checkpoint name from a node's parameters. falls back to the node id.
pub(super) fn parse_checkpoint_name(params: &Value, node_id: &str) -> String {
    params
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(node_id)
        .to_string()
}

/// process a checkpoint node: snapshot the current run state and active_node_id into an
/// automation_record row so a control-plane rollback api can restore the run to this point.
/// completes inline with no parking.
pub(super) async fn process_checkpoint_node<T: DatabaseImpl>(
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
    let params: Value = node.parameters.clone().into();
    let name = parse_checkpoint_name(&params, &node.id);
    let snapshot = runinator_models::json!({
        "name": name,
        "workflow_run_id": workflow_run.id,
        "active_node_id": workflow_run.active_node_id,
        "run_state": workflow_run.state,
        "captured_at": Utc::now().timestamp(),
    });
    let inserted = db
        .create_automation_record(RECORD_TYPE.into(), snapshot)
        .await?;
    let checkpoint_id = inserted
        .get("id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<Uuid>().ok());
    let output = CheckpointOutput {
        name,
        checkpoint_id,
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some("checkpoint_saved".into()),
        node_runs,
    )
    .await?;
    Ok(())
}
