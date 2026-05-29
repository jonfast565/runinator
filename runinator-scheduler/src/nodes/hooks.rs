// cross-cutting observers fired around every node, independent of node behavior.
//
// node handlers decide control flow; hooks watch it. this is where logging, metrics, auditing, or
// event emission live so they are not copied into each handler. pause and cancel are workflow-
// scoped (handled by the pre-dispatch gates), so they surface here as observer events rather than
// `NodeHandler` methods.

use async_trait::async_trait;
use log::debug;
use runinator_models::value::Value;

use crate::nodes::context::NodeContext;

#[async_trait]
pub trait NodeLifecycleHook: Send + Sync {
    /// fresh work was kicked off for the node.
    async fn on_started(&self, _ctx: &NodeContext<'_>) {}
    /// the node settled successfully.
    async fn on_succeeded(&self, _ctx: &NodeContext<'_>, _output: Option<&Value>) {}
    /// the node settled with a failure / blocked status.
    async fn on_failed(&self, _ctx: &NodeContext<'_>, _message: Option<&str>) {}
    /// the node timed out.
    async fn on_timed_out(&self, _ctx: &NodeContext<'_>) {}
    /// the workflow moved on from the node to `target` (none when the run completed the workflow).
    async fn on_transition(&self, _ctx: &NodeContext<'_>, _target: Option<&str>) {}
    /// the run was paused at this node by a pause/debug request.
    async fn on_paused(&self, _ctx: &NodeContext<'_>) {}
    /// the run was canceled.
    async fn on_canceled(&self, _ctx: &NodeContext<'_>) {}
}

/// default hook that records lifecycle transitions at debug level.
pub struct TracingHook;

#[async_trait]
impl NodeLifecycleHook for TracingHook {
    async fn on_started(&self, ctx: &NodeContext<'_>) {
        debug!(
            "node started: run={} node={} kind={:?}",
            ctx.workflow_run.id, ctx.node.id, ctx.node.kind
        );
    }

    async fn on_succeeded(&self, ctx: &NodeContext<'_>, _output: Option<&Value>) {
        debug!(
            "node succeeded: run={} node={}",
            ctx.workflow_run.id, ctx.node.id
        );
    }

    async fn on_failed(&self, ctx: &NodeContext<'_>, message: Option<&str>) {
        debug!(
            "node failed: run={} node={} message={:?}",
            ctx.workflow_run.id, ctx.node.id, message
        );
    }

    async fn on_timed_out(&self, ctx: &NodeContext<'_>) {
        debug!(
            "node timed out: run={} node={}",
            ctx.workflow_run.id, ctx.node.id
        );
    }

    async fn on_transition(&self, ctx: &NodeContext<'_>, target: Option<&str>) {
        debug!(
            "node transition: run={} node={} target={:?}",
            ctx.workflow_run.id, ctx.node.id, target
        );
    }

    async fn on_paused(&self, ctx: &NodeContext<'_>) {
        debug!(
            "node paused: run={} node={}",
            ctx.workflow_run.id, ctx.node.id
        );
    }

    async fn on_canceled(&self, ctx: &NodeContext<'_>) {
        debug!(
            "node canceled: run={} node={}",
            ctx.workflow_run.id, ctx.node.id
        );
    }
}

/// fan a single lifecycle event out to a slice of hooks.
pub async fn fire_outcome(
    hooks: &[std::sync::Arc<dyn NodeLifecycleHook>],
    ctx: &NodeContext<'_>,
    outcome: &crate::nodes::handler::NodeOutcome,
) {
    use crate::nodes::handler::NodeOutcome;
    use runinator_models::workflows::WorkflowStatus;
    for hook in hooks {
        match outcome {
            NodeOutcome::Started => hook.on_started(ctx).await,
            NodeOutcome::Pending => {}
            NodeOutcome::Retrying => hook.on_failed(ctx, Some("retrying")).await,
            NodeOutcome::Blocked => hook.on_failed(ctx, Some("blocked")).await,
            NodeOutcome::Advanced { status, target } => {
                match status {
                    WorkflowStatus::Succeeded | WorkflowStatus::Running => {
                        hook.on_succeeded(ctx, None).await
                    }
                    WorkflowStatus::TimedOut => hook.on_timed_out(ctx).await,
                    WorkflowStatus::Canceled => hook.on_canceled(ctx).await,
                    _ => hook.on_failed(ctx, None).await,
                }
                hook.on_transition(ctx, target.as_deref()).await;
            }
        }
    }
}
