// poller nodes: they kick off work, park the run, and re-enter each tick until it settles.
//
// action follows the default lifecycle template (on_enter + on_timeout); the others override
// `process` because their historical re-entrancy differs from the generic poller shape.

use async_trait::async_trait;
use runinator_comm::WireCodec;
use runinator_models::value::Value;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    workflow_state::{
        ActionIdempotencyRecord, ApprovalRecord, ApprovalState, JoinOutput, WaitElapsedOutput,
        WaitState,
    },
    workflows::{WorkflowNodeKind, WorkflowStatus},
};

use crate::context::build_node_parameters;
use crate::nodes::context::NodeContext;
use crate::nodes::handler::{NodeHandler, NodeOutcome};
use crate::nodes::run_state::{branch_policy_name, join_satisfied};

pub struct ActionHandler;

#[async_trait]
impl NodeHandler for ActionHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Action
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let action = ctx.node.action.as_ref().ok_or_else(|| {
            Box::new(RuntimeError::new(
                "workflow.node.action_missing".into(),
                format!("Action node {} has no action configuration", ctx.node.id),
            )) as SendableError
        })?;
        // reuse a queued run from a prior retry, otherwise start a fresh attempt.
        let node_run = match ctx.latest_with_status(WorkflowStatus::Queued) {
            Some(node_run) => node_run.clone(),
            None => ctx.create_node_run().await?,
        };
        let parameters = build_node_parameters(action, ctx.node, ctx.workflow_run, ctx.node_runs)?;
        let attempt = node_run.attempt + 1;
        let idempotency_scope = "workflow_action_node";
        let idempotency_key =
            workflow_task_idempotency_key(ctx.workflow_run.id, &ctx.node.id, node_run.id, attempt);
        if ctx
            .api
            .fetch_idempotency_key(idempotency_scope, &idempotency_key)
            .await?
            .is_none()
        {
            crate::iteration::enqueue_action_with_dedupe(
                ctx.api,
                ctx.workflow_run.id,
                &node_run,
                action,
                parameters.clone(),
                format!("workflow-node-run:{}", node_run.id),
            )
            .await?;
            let record = ActionIdempotencyRecord {
                workflow_node_run_id: node_run.id,
            };
            ctx.api
                .put_idempotency_key(idempotency_scope, &idempotency_key, record.to_wire_value()?)
                .await?;
        }

        ctx.update_node_run(
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
        ctx.update_run(
            WorkflowStatus::Running,
            Some(ctx.node.id.clone()),
            None,
            None,
        )
        .await?;
        Ok(NodeOutcome::Started)
    }

    async fn on_timeout(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let Some(node_run) = ctx.latest else {
            return Ok(NodeOutcome::Pending);
        };
        // actively cancel the task on the worker before timing the node out.
        if let Some(broker) = ctx.broker {
            let _ = broker
                .publish_control(runinator_broker::ControlCommand::new(
                    ctx.workflow_run.id,
                    runinator_comm::ControlKind::Cancel,
                ))
                .await;
        }
        ctx.time_out(node_run, "Node timed out").await
    }
}

/// idempotency key for an action node's enqueue, scoped to the attempt.
pub(crate) fn workflow_task_idempotency_key(
    workflow_run_id: i64,
    node_id: &str,
    workflow_node_run_id: i64,
    attempt: i64,
) -> String {
    format!("{workflow_run_id}:{node_id}:{workflow_node_run_id}:{attempt}")
}

pub struct WaitHandler;

#[async_trait]
impl NodeHandler for WaitHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Wait
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let params = runinator_workflows::parse_wait_parameters(ctx.node);
        if let Some(node_run) = ctx.latest_with_status(WorkflowStatus::Waiting) {
            if ctx.timed_out(node_run) {
                return ctx.time_out(node_run, "Wait node timed out").await;
            }
            let wait_state = WaitState::from_wire_value(&node_run.state).ok();
            if let Some(expected) = params.until_status.as_deref() {
                let current = wait_state
                    .as_ref()
                    .map(|state| state.status.as_str())
                    .unwrap_or_default();
                if current == expected {
                    return ctx
                        .transition(
                            node_run,
                            WorkflowStatus::Succeeded,
                            Some(node_run.state.clone()),
                            Some("wait_status_matched".into()),
                        )
                        .await;
                }
                return Ok(NodeOutcome::Pending);
            }
            let deadline = wait_state
                .as_ref()
                .map(|state| state.deadline_unix)
                .unwrap_or(i64::MAX);
            if chrono::Utc::now().timestamp() < deadline {
                return Ok(NodeOutcome::Pending);
            }
            let output = WaitElapsedOutput {
                deadline_unix: deadline,
            };
            return ctx
                .transition(
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("wait_elapsed".into()),
                )
                .await;
        }
        let deadline = chrono::Utc::now().timestamp() + params.seconds;
        let state = WaitState {
            deadline_unix: deadline,
            status: params.initial_status,
        };
        let state_value = state.to_wire_value()?;
        let node_run = ctx.create_node_run().await?;
        ctx.update_node_run(
            node_run.id,
            WorkflowStatus::Waiting,
            Some(node_run.attempt + 1),
            None,
            None,
            Some(state_value.clone()),
            Some("wait_started".into()),
            None,
        )
        .await?;
        ctx.update_run(
            WorkflowStatus::Waiting,
            Some(ctx.node.id.clone()),
            Some(state_value),
            None,
        )
        .await?;
        Ok(NodeOutcome::Started)
    }
}

pub struct ApprovalHandler;

#[async_trait]
impl NodeHandler for ApprovalHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Approval
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        if let Some(node_run) = ctx.latest {
            if node_run.status == WorkflowStatus::ApprovalRequired && ctx.timed_out(node_run) {
                return ctx.time_out(node_run, "Approval timed out").await;
            }
            if node_run.status == WorkflowStatus::Succeeded {
                return ctx
                    .transition(
                        node_run,
                        WorkflowStatus::Succeeded,
                        node_run.output_json.clone(),
                        Some("approval_resolved".into()),
                    )
                    .await;
            }
            return Ok(NodeOutcome::Pending);
        }
        let node_run = ctx.create_node_run().await?;
        let params = runinator_workflows::parse_approval_parameters(ctx.node);
        let record = ApprovalRecord {
            workflow_run_id: ctx.workflow_run.id,
            node_id: ctx.node.id.clone(),
            approval_type: params.approval_type,
            prompt: params.prompt,
            status: "pending".into(),
            provider: "runinator".into(),
            resource_type: "approval_request".into(),
            external_id: format!("workflow:{}:node:{}", ctx.workflow_run.id, ctx.node.id),
            metadata: params.metadata,
        };
        let approval = ctx
            .api
            .create_automation_record("/approvals", record.to_wire_value()?)
            .await?;
        let approval_state = ApprovalState {
            approval: ctx.node.parameters.clone(),
            approval_id: approval.get("id").and_then(Value::as_i64),
        };
        ctx.update_node_run(
            node_run.id,
            WorkflowStatus::ApprovalRequired,
            Some(node_run.attempt + 1),
            None,
            None,
            Some(approval_state.to_wire_value()?),
            Some("approval_required".into()),
            None,
        )
        .await?;
        ctx.update_run(
            WorkflowStatus::ApprovalRequired,
            Some(ctx.node.id.clone()),
            None,
            None,
        )
        .await?;
        Ok(NodeOutcome::Started)
    }
}

pub struct JoinHandler;

#[async_trait]
impl NodeHandler for JoinHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Join
    }

    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let params = runinator_workflows::parse_join_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let wait_for = params
            .wait_for
            .iter()
            .map(|target| target.as_str().to_string())
            .collect::<Vec<_>>();
        if join_satisfied(&wait_for, params.mode, ctx.node_runs) {
            let node_run = ctx.ensure_node_run().await?;
            let output = JoinOutput {
                wait_for,
                mode: branch_policy_name(params.mode).to_string(),
            };
            return ctx
                .transition(
                    &node_run,
                    WorkflowStatus::Succeeded,
                    Some(output.to_wire_value()?),
                    Some("join_satisfied".into()),
                )
                .await;
        }
        if let Some(node_run) = ctx.latest_with_status(WorkflowStatus::Waiting)
            && ctx.timed_out(node_run)
        {
            return ctx.time_out(node_run, "Join node timed out").await;
        }
        let mut state = ctx.run_state();
        if let Some(target) = state.pop_parallel_remaining() {
            return ctx
                .goto(
                    target,
                    Some(state.into_value()?),
                    Some("join_waiting_for_parallel_branch".into()),
                )
                .await;
        }
        let node_run = ctx.ensure_node_run().await?;
        ctx.update_node_run(
            node_run.id,
            WorkflowStatus::Waiting,
            Some(node_run.attempt + 1),
            None,
            None,
            None,
            Some("join_waiting".into()),
            None,
        )
        .await?;
        ctx.update_run(
            WorkflowStatus::Waiting,
            Some(ctx.node.id.clone()),
            None,
            None,
        )
        .await?;
        Ok(NodeOutcome::Started)
    }
}
