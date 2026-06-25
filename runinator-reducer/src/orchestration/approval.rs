use super::context::is_reentry_stale;
use super::transitions::{arm_node_timeout, time_out, timed_out, transition_from_node};
use super::*;

pub(super) async fn process_approval_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    // a loop body re-entering this node sees the prior iteration's resolved run; treat it as a
    // fresh visit so a new approval is requested instead of transitioning from the stale run.
    let latest = latest.filter(|run| !is_reentry_stale(run, node_runs));
    if let Some(node_run) = latest {
        if node_run.status == WorkflowStatus::ApprovalRequired && timed_out(node, node_run) {
            return time_out(
                db,
                workflow_run,
                node,
                node_run,
                "Approval timed out",
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
                Some("approval_resolved".into()),
                node_runs,
            )
            .await?;
            return Ok(());
        }
        return Ok(());
    }
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    let params = runinator_workflows::parse_approval_parameters(node);
    let record = ApprovalRecord {
        workflow_run_id: workflow_run.id,
        node_id: node.id.clone(),
        approval_type: params.approval_type,
        prompt: params.prompt,
        status: "pending".into(),
        provider: "runinator".into(),
        resource_type: "approval_request".into(),
        external_id: format!("workflow:{}:node:{}", workflow_run.id, node.id),
        metadata: params.metadata,
    };
    let approval = db
        .create_automation_record("approval_requests".into(), record.to_wire_value()?)
        .await?;
    let approval_state = ApprovalState {
        approval: node.parameters.clone().into(),
        approval_id: approval
            .get("id")
            .and_then(Value::as_str)
            .and_then(|raw| raw.parse::<Uuid>().ok()),
    };
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::ApprovalRequired,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(approval_state.to_wire_value()?),
        Some(WorkflowStatus::ApprovalRequired.as_str().into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::ApprovalRequired,
        Some(node.id.clone()),
        None,
        None,
    )
    .await?;
    arm_node_timeout(db, workflow_run.id, node).await
}

pub(super) struct ApprovalHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for ApprovalHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move {
            process_approval_node(
                ctx.db,
                ctx.workflow_run,
                ctx.node,
                ctx.latest,
                ctx.node_runs,
            )
            .await?;
            Ok(ReadyNodeDisposition::Complete)
        }
    }
}
