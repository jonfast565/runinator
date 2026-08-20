use runinator_models::{
    cursor::RunCursor,
    errors::SendableError,
    workflow_state::WorkflowRunState,
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeRun, WorkflowRun},
};
use runinator_store::RuntimeStore;
use std::ops::Deref;

use super::ReadyNodeDisposition;

/// shared references for operations scoped to one workflow run.
pub(super) struct WorkflowRunContext<'a, T: RuntimeStore> {
    pub db: &'a T,
    pub workflow_run: &'a WorkflowRun,
}

impl<'a, T: RuntimeStore> WorkflowRunContext<'a, T> {
    pub(super) fn new(db: &'a T, workflow_run: &'a WorkflowRun) -> Self {
        Self { db, workflow_run }
    }
}

/// run and cursor references used to build a step's expression context.
pub(super) struct RunStepContext<'a, T: RuntimeStore> {
    pub(super) run: WorkflowRunContext<'a, T>,
    /// the thread of control this step advances. operations that reason about the run's placement
    /// should read this rather than `workflow_run.active_node_id`, which mirrors only the primary
    /// cursor and carries no start-node fallback.
    pub cursor: &'a RunCursor,
    pub node_runs: &'a [WorkflowNodeRun],
}

impl<'a, T: RuntimeStore> RunStepContext<'a, T> {
    pub(super) fn new(
        run: WorkflowRunContext<'a, T>,
        cursor: &'a RunCursor,
        node_runs: &'a [WorkflowNodeRun],
    ) -> Self {
        Self {
            run,
            cursor,
            node_runs,
        }
    }
}

/// shared references for one runtime step, including interpreter gates such as debugging.
pub(super) struct NodeStepContext<'a, T: RuntimeStore> {
    run: RunStepContext<'a, T>,
    pub workflow: &'a WorkflowDefinition,
    pub node: &'a WorkflowNode,
    pub latest: Option<&'a WorkflowNodeRun>,
    /// all validated nodes in the workflow; available to operations that must resolve
    /// cross-node references (compute, subflow, compensation).
    pub nodes: &'a [WorkflowNode],
    run_state_snapshot: WorkflowRunState,
}

/// the immutable node and cursor references needed by deadline calculations.
#[derive(Clone, Copy)]
pub(super) struct NodeTimingContext<'a> {
    pub node: &'a WorkflowNode,
    pub cursor: &'a RunCursor,
}

impl<'a> NodeTimingContext<'a> {
    pub(super) fn new(node: &'a WorkflowNode, cursor: &'a RunCursor) -> Self {
        Self { node, cursor }
    }
}

impl<'a, T: RuntimeStore> NodeStepContext<'a, T> {
    pub(super) fn new(
        run: RunStepContext<'a, T>,
        workflow: &'a WorkflowDefinition,
        node: &'a WorkflowNode,
        latest: Option<&'a WorkflowNodeRun>,
        nodes: &'a [WorkflowNode],
    ) -> Self {
        let run_state_snapshot = run.workflow_run.execution_state.clone();
        Self {
            run,
            workflow,
            node,
            latest,
            nodes,
            run_state_snapshot,
        }
    }

    pub(super) fn with_node_runs<'b>(
        &'b self,
        node_runs: &'b [WorkflowNodeRun],
    ) -> NodeStepContext<'b, T> {
        NodeStepContext::new(
            RunStepContext::new(
                WorkflowRunContext::new(self.db, self.workflow_run),
                self.cursor,
                node_runs,
            ),
            self.workflow,
            self.node,
            self.latest,
            self.nodes,
        )
    }

    pub(super) fn timing(&self) -> NodeTimingContext<'_> {
        NodeTimingContext::new(self.node, self.cursor)
    }

    /// Typed, read-only state captured at the start of this runtime step.
    pub(super) fn run_state_snapshot(&self) -> &WorkflowRunState {
        &self.run_state_snapshot
    }
}

impl<'a, T: RuntimeStore> Deref for NodeStepContext<'a, T> {
    type Target = RunStepContext<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.run
    }
}

impl<'a, T: RuntimeStore> Deref for RunStepContext<'a, T> {
    type Target = WorkflowRunContext<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.run
    }
}

/// All context a node operation needs to process a single interpreter step.
///
/// This alias keeps handler signatures descriptive without inserting another wrapper around the
/// actual node-step data.
pub(super) type NodeExecutionContext<'a, T> = NodeStepContext<'a, T>;

pub(super) fn complete(
    result: Result<(), SendableError>,
) -> Result<ReadyNodeDisposition, SendableError> {
    result?;
    Ok(ReadyNodeDisposition::Complete)
}
