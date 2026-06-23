use super::transitions::transition_from_node;
use super::*;

pub(super) async fn process_wait_node<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
) -> Result<ReadyNodeDisposition, SendableError> {
    let params = runinator_workflows::parse_wait_parameters(node);
    if let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) {
        let wait_state = serde_json::from_value::<WaitState>(node_run.state.clone().into()).ok();
        let deadline = wait_state
            .as_ref()
            .map(|state| state.deadline_unix)
            .unwrap_or(i64::MAX);
        if Utc::now().timestamp() < deadline {
            return Ok(ReadyNodeDisposition::KeepClaim);
        }
        let output = WaitElapsedOutput {
            deadline_unix: deadline,
        };
        let node_runs = db.fetch_workflow_node_runs(workflow_run.id).await?;
        transition_from_node(
            db,
            workflow_run,
            node,
            node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("wait_elapsed".into()),
            &node_runs,
        )
        .await?;
        return Ok(ReadyNodeDisposition::Complete);
    }

    let deadline = Utc::now().timestamp() + params.seconds;
    let state = WaitState {
        deadline_unix: deadline,
        status: params.initial_status,
    }
    .to_wire_value()?;
    let node_run = db
        .create_workflow_node_run(
            workflow_run.id,
            node.id.clone(),
            node.parameters.clone().into(),
        )
        .await?;
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Waiting,
        Some(node_run.attempt + 1),
        None,
        None,
        Some(state.clone()),
        Some("wait_started".into()),
        None,
    )
    .await?;
    db.update_workflow_run_status(
        workflow_run.id,
        WorkflowStatus::Waiting,
        Some(node.id.clone()),
        Some(state),
        None,
    )
    .await?;
    let ready_at = chrono::DateTime::<Utc>::from_timestamp(deadline, 0).unwrap_or_else(Utc::now);
    let event = runinator_models::orchestration::NewOrchestrationEvent::new(
        workflow_run.id,
        Some(node.id.clone()),
        "node_waiting",
        runinator_models::json!({ "node_id": node.id }),
    );
    db.enqueue_ready_node(event, node.id.clone(), ready_at)
        .await?;
    Ok(ReadyNodeDisposition::Complete)
}

pub(super) struct WaitHandler;

impl<T: DatabaseImpl> super::handler::NodeHandler<T> for WaitHandler {
    fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> impl std::future::Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a,
    {
        async move { process_wait_node(ctx.db, ctx.workflow_run, ctx.node, ctx.latest).await }
    }
}
