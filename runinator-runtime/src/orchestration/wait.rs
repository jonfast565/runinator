use super::transitions::transition_from_node;
use super::*;

/// has a parked wait's deadline passed?
///
/// shared with the interrupt layer, which binds the `wake` source to exactly this condition. one
/// definition so "the wait is up" cannot mean two different things to the two callers.
pub(super) fn deadline_elapsed(latest: Option<&WorkflowNodeRun>) -> bool {
    let Some(node_run) = latest.filter(|run| run.status == WorkflowStatus::Waiting) else {
        return false;
    };
    let deadline = node_run
        .state
        .decode::<WaitState>()
        .map(|state| state.deadline_unix)
        .unwrap_or(i64::MAX);
    Utc::now().timestamp() >= deadline
}

pub(super) struct WaitOp;

impl WaitOp {
    pub(super) async fn process<T: RuntimeStore>(
        &self,
        ctx: &super::execution::NodeExecutionContext<'_, T>,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let params = runinator_workflows::parse_wait_parameters(ctx.node);
        if let Some(node_run) = ctx
            .latest
            .filter(|run| run.status == WorkflowStatus::Waiting)
        {
            let wait_state = node_run.state.decode::<WaitState>().ok();
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
            let node_runs = ctx.db.fetch_workflow_node_runs(ctx.workflow_run.id).await?;
            let transition_ctx = ctx.with_node_runs(&node_runs);
            transition_from_node(
                &transition_ctx,
                node_run,
                WorkflowStatus::Succeeded,
                Some(output.to_wire_value()?),
                Some("wait_elapsed".into()),
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
        ctx.db
            .update_workflow_node_run(
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
        ctx.db
            .update_workflow_run_status(
                ctx.workflow_run.id,
                WorkflowStatus::Waiting,
                Some(ctx.node.id.clone()),
                None,
                None,
            )
            .await?;
        let ready_at =
            chrono::DateTime::<Utc>::from_timestamp(deadline, 0).unwrap_or_else(Utc::now);
        let event = runinator_models::orchestration::NewOrchestrationEvent::new(
            ctx.workflow_run.id,
            Some(ctx.node.id.clone()),
            "node_waiting",
            runinator_models::json!({ "node_id": ctx.node.id }),
        );
        ctx.db
            .enqueue_ready_node(event, ctx.node.id.clone(), ready_at)
            .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
