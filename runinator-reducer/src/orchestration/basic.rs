use super::context::runtime_context;
use super::transitions::{ensure_node_run, transition_from_node};
use super::*;

pub(super) async fn process_config_node<T: DatabaseImpl>(
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
    let context = runtime_context(db, workflow_run, node_runs).await;
    let resolved = runinator_workflows::resolve_value_refs(&node.parameters, &context)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let new_name = resolved.get("name").and_then(|value| match value {
        Value::Null => None,
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        other => Some(other.to_string()),
    });
    if new_name.is_some() {
        db.set_workflow_run_name(workflow_run.id, new_name.clone())
            .await?;
    }
    let summary = ConfigSummary {
        name: new_name,
        metadata: resolved.get("metadata").cloned(),
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(summary.to_wire_value()?),
        Some("config_applied".into()),
        node_runs,
    )
    .await?;
    Ok(())
}

pub(super) async fn process_skipped_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let node_run = ensure_node_run(db, workflow_run, node, latest).await?;
    let output = SkippedOutput {
        skipped: true,
        node_id: node.id.clone(),
    };
    transition_from_node(
        db,
        workflow_run,
        node,
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some(format!("Node {} skipped", node.id)),
        node_runs,
    )
    .await?;
    Ok(())
}

// --- rich control-flow nodes -------------------------------------------------
//
// the reducer lives here and calls `DatabaseImpl` directly. control-flow bookkeeping lives in
// named frames inside `workflow_run.state` (the typed `WorkflowRunState` from runinator-models).
// predicates that read sibling node-run history come from runinator-workflows.
