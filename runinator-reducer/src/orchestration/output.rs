use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;
use crate::errors::ARTIFACT_SOURCE_UNRESOLVED;
use runinator_models::json;
use runinator_models::workflows::NewWorkflowRunArtifact;

pub(super) struct OutputHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for OutputHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let params = runinator_workflows::parse_output_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let context = runtime_context(ctx).await;
        let data = runinator_workflows::evaluate_expression(&params.data, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;

        // emit an automation event only when an event_type is declared.
        if let Some(ref event_type) = params.event_type {
            let message = format!("Output {}", event_type);
            ctx.db
                .create_automation_record(
                    "automation_events".into(),
                    json!({
                        "workflow_run_id": ctx.workflow_run.id,
                        "node_id": ctx.node.id,
                        "provider": "runinator",
                        "resource_type": "automation_event",
                        "external_id": node_run.id,
                        "status": "output_recorded",
                        "event_type": event_type.clone(),
                        "message": message,
                        "metadata": {
                            "workflow_node_run_id": node_run.id,
                            "workflow_id": ctx.workflow_run.workflow_id,
                            "data": data.clone()
                        }
                    }),
                )
                .await?;
        }

        // promote artifact items to run-level artifacts.
        let mut artifacts = Vec::new();
        for item in &params.items {
            let resolved = runinator_workflows::evaluate_expression(&item.source, &context)
                .map_err(|err| -> SendableError { Box::new(err) })?;
            for artifact_value in artifact_values(&resolved) {
                let new_artifact = build_artifact(
                    ctx.workflow_run.id,
                    &ctx.node.id,
                    &item.name,
                    artifact_value,
                )?;
                let stored = ctx.db.add_workflow_run_artifact(&new_artifact).await?;
                artifacts.push(json!({
                    "id": stored.id,
                    "name": stored.name,
                    "artifact_id": stored.artifact_id,
                    "mime_type": stored.mime_type,
                    "size_bytes": stored.size_bytes,
                    "uri": stored.uri,
                }));
            }
        }

        let output = OutputPayload {
            event_type: params.event_type,
            data,
            artifacts,
        };
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("output_recorded".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}

// flatten a resolved source into the artifact descriptors it carries (single object or array).
fn artifact_values(resolved: &Value) -> Vec<&Value> {
    match resolved {
        Value::Array(items) => items.iter().collect(),
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

fn build_artifact(
    workflow_run_id: Uuid,
    node_id: &str,
    name: &str,
    artifact: &Value,
) -> Result<NewWorkflowRunArtifact, SendableError> {
    let object = artifact
        .as_object()
        .ok_or_else(|| ARTIFACT_SOURCE_UNRESOLVED.error(format!("'{name}' is not an artifact")))?;
    let artifact_id = object
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| ARTIFACT_SOURCE_UNRESOLVED.error(format!("'{name}' has no artifact id")))?;
    let uri = object
        .get("uri")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ARTIFACT_SOURCE_UNRESOLVED.error(format!("'{name}' has no uri")))?;
    let mime_type = object
        .get("mime_type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_string();
    let size_bytes = object
        .get("size_bytes")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let metadata = object.get("metadata").cloned().unwrap_or(Value::Null);
    Ok(NewWorkflowRunArtifact {
        workflow_run_id,
        node_id: node_id.to_string(),
        artifact_id,
        name: name.to_string(),
        mime_type,
        size_bytes,
        uri,
        metadata,
    })
}
