use std::future::Future;

use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeRun, WorkflowRun},
};

use super::ReadyNodeDisposition;

/// all context a node handler needs to process a single reducer step.
pub(super) struct NodeHandlerContext<'a, T: DatabaseImpl> {
    pub db: &'a T,
    pub workflow: &'a WorkflowDefinition,
    pub workflow_run: &'a WorkflowRun,
    pub node: &'a WorkflowNode,
    pub latest: Option<&'a WorkflowNodeRun>,
    pub node_runs: &'a [WorkflowNodeRun],
    /// all validated nodes in the workflow; available to handlers that must resolve
    /// cross-node references (compute, subflow, compensation).
    pub nodes: &'a [WorkflowNode],
}

/// the processing contract every node kind must fulfill.
///
/// implementors return `KeepClaim` when the workflow must stay parked (e.g. a timer
/// that has not yet elapsed) and `Complete` in all other cases.
pub(super) trait NodeHandler<T: DatabaseImpl> {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a;
}
