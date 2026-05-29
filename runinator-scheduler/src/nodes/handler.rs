// the per-node-type behavior contract.
//
// the workflow engine is re-entrant: `process` is called once per scheduling tick for whichever
// node is active, and the handler inspects the latest run to decide what to do. the trait offers a
// decomposed lifecycle (on_enter / poll / on_timeout / on_settled) with a default `process` that
// orchestrates them in the common poller shape — implement just those for a new node. handlers
// whose historical flow differs (the control-flow nodes) override `process` wholesale and drive the
// `NodeContext` helpers directly.

use async_trait::async_trait;
use runinator_models::{
    errors::SendableError, workflows::WorkflowNodeKind, workflows::WorkflowStatus,
};

use crate::nodes::context::NodeContext;

/// what a handler did this tick. the driver maps it onto the lifecycle hooks; it does not drive
/// persistence itself (the `NodeContext` helpers already did that).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeOutcome {
    /// fresh work was kicked off, or the node parked itself waiting.
    Started,
    /// in-flight work; nothing changed this tick.
    Pending,
    /// the node settled and the workflow advanced. `target` is the node moved to, or none when the
    /// run reached a terminal state.
    Advanced {
        status: WorkflowStatus,
        target: Option<String>,
    },
    /// the run failed but was requeued for another attempt.
    Retrying,
    /// the workflow was blocked.
    Blocked,
}

#[async_trait]
pub trait NodeHandler: Send + Sync {
    /// the node kind this handler serves; used for registry lookup.
    fn kind(&self) -> WorkflowNodeKind;

    /// stateless nodes ignore prior runs and always re-evaluate from `on_enter`. control-flow and
    /// in-flight nodes leave this false so the default `process` consults the latest run.
    fn synchronous(&self) -> bool {
        false
    }

    /// fresh activation: no in-flight run yet (or a queued run awaiting kickoff). start the work.
    /// nodes that fully override `process` (control-flow nodes) never reach this default.
    async fn on_enter(&self, _ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        Ok(NodeOutcome::Pending)
    }

    /// re-entry while a run is in flight (Running / Waiting / ApprovalRequired) and not timed out.
    /// default: keep waiting.
    async fn poll(&self, _ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        Ok(NodeOutcome::Pending)
    }

    /// the in-flight run exceeded `node.timeout_seconds`. default: time out, retrying if attempts
    /// remain. override to add side effects (e.g. cancelling a worker).
    async fn on_timeout(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        let Some(run) = ctx.latest else {
            return Ok(NodeOutcome::Pending);
        };
        ctx.time_out(run, "Node timed out").await
    }

    /// the in-flight run reached a terminal status (e.g. a worker reported success/failure).
    /// default mirrors the action node: success transitions, failure retries then transitions.
    async fn on_settled(
        &self,
        ctx: &NodeContext<'_>,
        status: WorkflowStatus,
    ) -> Result<NodeOutcome, SendableError> {
        let Some(run) = ctx.latest else {
            return Ok(NodeOutcome::Pending);
        };
        match status {
            WorkflowStatus::Succeeded => {
                ctx.transition(run, status, run.output_json.clone(), None)
                    .await
            }
            _ => {
                ctx.retry_or_transition(run, status, run.output_json.clone(), run.message.clone())
                    .await
            }
        }
    }

    /// the full per-tick driver. the default orchestrates the lifecycle above; override for nodes
    /// with bespoke re-entrancy.
    async fn process(&self, ctx: &NodeContext<'_>) -> Result<NodeOutcome, SendableError> {
        if self.synchronous() {
            return self.on_enter(ctx).await;
        }
        let Some(run) = ctx.latest else {
            return self.on_enter(ctx).await;
        };
        if run.status.is_terminal() {
            return self.on_settled(ctx, run.status).await;
        }
        match run.status {
            WorkflowStatus::Running
            | WorkflowStatus::Waiting
            | WorkflowStatus::ApprovalRequired => {
                if ctx.timed_out(run) {
                    self.on_timeout(ctx).await
                } else {
                    self.poll(ctx).await
                }
            }
            WorkflowStatus::Queued => self.on_enter(ctx).await,
            _ => Ok(NodeOutcome::Pending),
        }
    }
}
