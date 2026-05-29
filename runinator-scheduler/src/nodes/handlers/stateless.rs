// synchronous nodes: they create a run, evaluate, and settle within a single tick. they ignore any
// prior run (synchronous = true) so loop/reentry re-evaluates them each visit.

use async_trait::async_trait;
use runinator_comm::WireCodec;
use runinator_models::{
    errors::SendableError,
    workflow_state::{ConfigSummary, EmitOutput, SwitchOutput},
    workflows::{WorkflowNodeKind, WorkflowStatus},
};
use serde_json::Value;

use crate::nodes::context::NodeContext;
use crate::nodes::driver;
use crate::nodes::handler::{NodeHandler, NodeOutcome};
use crate::nodes::run_state::RunState;

pub struct StartHandler;

#[async_trait]
impl NodeHandler for StartHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Start
    }

    fn synchronous(&self) -> bool {
        true
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let node_run = ctx.ensure_node_run().await?;
        ctx.transition(
            &node_run,
            WorkflowStatus::Succeeded,
            None,
            Some("start_reached".into()),
        )
        .await
    }
}

pub struct ConditionHandler;

#[async_trait]
impl NodeHandler for ConditionHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Condition
    }

    fn synchronous(&self) -> bool {
        true
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let node_run = ctx.create_node_run().await?;
        let context = ctx.runtime_context();
        let matched = runinator_workflows::evaluate_condition(&ctx.node.condition, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let (status, reason) = if matched {
            (WorkflowStatus::Succeeded, "condition_matched")
        } else {
            (WorkflowStatus::Blocked, "condition_unmatched")
        };
        ctx.transition(&node_run, status, None, Some(reason.into()))
            .await
    }
}

pub struct SwitchHandler;

#[async_trait]
impl NodeHandler for SwitchHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Switch
    }

    fn synchronous(&self) -> bool {
        true
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let node_run = ctx.create_node_run().await?;
        let params = runinator_workflows::parse_switch_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let context = ctx.runtime_context();
        let target = runinator_workflows::evaluate_switch(&params, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let output = SwitchOutput {
            target: target.clone(),
        }
        .to_wire_value()?;
        ctx.update_node_run(
            node_run.id,
            if target.is_some() {
                WorkflowStatus::Succeeded
            } else {
                WorkflowStatus::Blocked
            },
            Some(node_run.attempt + 1),
            None,
            Some(output),
            None,
            Some("switch_evaluated".into()),
            None,
        )
        .await?;
        match target {
            Some(target) => ctx.goto(target, None, None).await,
            None => {
                ctx.transition(
                    &node_run,
                    WorkflowStatus::Blocked,
                    None,
                    Some("Switch did not match a target".into()),
                )
                .await
            }
        }
    }
}

pub struct EmitHandler;

#[async_trait]
impl NodeHandler for EmitHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Emit
    }

    fn synchronous(&self) -> bool {
        true
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let node_run = ctx.create_node_run().await?;
        let params = runinator_workflows::parse_emit_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let context = ctx.runtime_context();
        let data = runinator_workflows::resolve_value_refs(&params.data, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let output = EmitOutput {
            event_type: params.event_type,
            data,
        };
        ctx.transition(
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("emit_recorded".into()),
        )
        .await
    }
}

pub struct ConfigHandler;

#[async_trait]
impl NodeHandler for ConfigHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Config
    }

    fn synchronous(&self) -> bool {
        true
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let node_run = ctx.create_node_run().await?;
        let context = ctx.runtime_context();
        let resolved = runinator_workflows::resolve_value_refs(&ctx.node.parameters, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;

        let new_name = resolved.get("name").and_then(|value| match value {
            Value::Null => None,
            Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
            other => Some(other.to_string()),
        });
        let metadata_patch = resolved.get("metadata").cloned();

        if new_name.is_some() {
            ctx.api
                .set_workflow_run_name(ctx.workflow_run.id, new_name.clone())
                .await?;
        }

        let summary = ConfigSummary {
            name: new_name.clone(),
            metadata: metadata_patch.clone(),
        };

        // merge metadata into the run's state.run_metadata bag.
        if let Some(metadata) = metadata_patch {
            let mut state = ctx.run_state();
            let merged_metadata = match state.run_metadata() {
                Some(existing) => driver::merge_json(existing.clone(), metadata),
                None => metadata,
            };
            state.set_run_metadata(merged_metadata);
            ctx.update_run(
                ctx.workflow_run.status,
                ctx.workflow_run.active_node_id.clone(),
                Some(state.into_value()?),
                None,
            )
            .await?;
        }

        ctx.transition(
            &node_run,
            WorkflowStatus::Succeeded,
            Some(summary.to_wire_value()?),
            Some("config_applied".into()),
        )
        .await
    }
}

pub struct EndHandler;

#[async_trait]
impl NodeHandler for EndHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::End
    }

    fn synchronous(&self) -> bool {
        true
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        driver::ensure_completed_node_run(
            ctx.api,
            ctx.workflow_run,
            ctx.node,
            ctx.latest,
            "end_reached",
        )
        .await?;
        // a loop body that runs through an end node must return to the loop, not terminate the run.
        let loop_return = ctx
            .run_state()
            .loop_frame()
            .map(|frame| frame.return_to.clone())
            .filter(|target| !target.is_empty());
        if let Some(loop_node) = loop_return {
            // reset the run state before re-entering; the loop node recomputes its frame from
            // node-run history, so any stale nested frames are intentionally dropped here.
            ctx.update_run(
                WorkflowStatus::Running,
                Some(loop_node.clone()),
                Some(RunState::default().into_value()?),
                None,
            )
            .await?;
            return Ok(NodeOutcome::Advanced {
                status: WorkflowStatus::Running,
                target: Some(loop_node),
            });
        }
        ctx.update_run(
            WorkflowStatus::Succeeded,
            Some(ctx.node.id.clone()),
            None,
            None,
        )
        .await?;
        Ok(NodeOutcome::Advanced {
            status: WorkflowStatus::Succeeded,
            target: None,
        })
    }
}

pub struct FailHandler;

#[async_trait]
impl NodeHandler for FailHandler {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Fail
    }

    fn synchronous(&self) -> bool {
        true
    }

    async fn on_enter(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        driver::ensure_completed_node_run(
            ctx.api,
            ctx.workflow_run,
            ctx.node,
            ctx.latest,
            "fail_reached",
        )
        .await?;
        ctx.update_run(
            WorkflowStatus::Failed,
            Some(ctx.node.id.clone()),
            None,
            Some("Workflow reached fail node".into()),
        )
        .await?;
        Ok(NodeOutcome::Advanced {
            status: WorkflowStatus::Failed,
            target: None,
        })
    }
}
