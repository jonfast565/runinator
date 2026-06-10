use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;
use runinator_models::json;

pub(super) async fn process_output_node<T: DatabaseImpl>(
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
    let params = runinator_workflows::parse_output_parameters(node)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let context = runtime_context(db, workflow_run, node_runs).await;
    let data = runinator_workflows::resolve_value_refs(&params.data, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let event_type = params
        .event_type
        .as_deref()
        .unwrap_or("workflow.output")
        .to_string();
    let message = format!("Output {}", event_type);
    db.create_automation_record(
        "automation_events".into(),
        json!({
            "workflow_run_id": workflow_run.id,
            "node_id": node.id,
            "provider": "runinator",
            "resource_type": "automation_event",
            "external_id": node_run.id,
            "status": "output_recorded",
            "event_type": event_type.clone(),
            "message": message,
            "metadata": {
                "workflow_node_run_id": node_run.id,
                "workflow_id": workflow_run.workflow_id,
                "data": data.clone()
            }
        }),
    )
    .await?;
    let output = OutputPayload {
        event_type: params.event_type,
        data,
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some("output_recorded".into()),
        node_runs,
    )
    .await?;
    Ok(())
}
