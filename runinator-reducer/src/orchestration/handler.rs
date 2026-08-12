use std::{future::Future, ops::Deref};

use runinator_models::{
    cursor::RunCursor,
    errors::SendableError,
    workflow_state::WorkflowRunState,
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeRun, WorkflowRun},
};
use runinator_store::ReducerStore;

use super::ReadyNodeDisposition;

/// shared references for operations scoped to one workflow run.
pub(super) struct WorkflowRunContext<'a, T: ReducerStore> {
    pub db: &'a T,
    pub workflow_run: &'a WorkflowRun,
}

impl<'a, T: ReducerStore> WorkflowRunContext<'a, T> {
    pub(super) fn new(db: &'a T, workflow_run: &'a WorkflowRun) -> Self {
        Self { db, workflow_run }
    }
}

/// run and cursor references used to build a step's expression context.
pub(super) struct RunStepContext<'a, T: ReducerStore> {
    pub(super) run: WorkflowRunContext<'a, T>,
    /// the thread of control this step advances. handlers that reason about the run's placement
    /// should read this rather than `workflow_run.active_node_id`, which mirrors only the primary
    /// cursor and carries no start-node fallback.
    pub cursor: &'a RunCursor,
    pub node_runs: &'a [WorkflowNodeRun],
}

impl<'a, T: ReducerStore> RunStepContext<'a, T> {
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

/// shared references for one reducer step, including pre-handler gates such as debugging.
pub(super) struct NodeStepContext<'a, T: ReducerStore> {
    run: RunStepContext<'a, T>,
    pub workflow: &'a WorkflowDefinition,
    pub node: &'a WorkflowNode,
    pub latest: Option<&'a WorkflowNodeRun>,
    /// all validated nodes in the workflow; available to handlers that must resolve
    /// cross-node references (compute, subflow, compensation).
    pub nodes: &'a [WorkflowNode],
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

impl<'a, T: ReducerStore> NodeStepContext<'a, T> {
    pub(super) fn new(
        run: RunStepContext<'a, T>,
        workflow: &'a WorkflowDefinition,
        node: &'a WorkflowNode,
        latest: Option<&'a WorkflowNodeRun>,
        nodes: &'a [WorkflowNode],
    ) -> Self {
        Self {
            run,
            workflow,
            node,
            latest,
            nodes,
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
}

impl<'a, T: ReducerStore> Deref for NodeStepContext<'a, T> {
    type Target = RunStepContext<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.run
    }
}

impl<'a, T: ReducerStore> Deref for RunStepContext<'a, T> {
    type Target = WorkflowRunContext<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.run
    }
}

/// all context a node handler needs to process a single reducer step.
pub(super) struct NodeHandlerContext<'a, T: ReducerStore> {
    step: NodeStepContext<'a, T>,
    /// typed state captured at handler dispatch. handlers may clone and modify this snapshot, but
    /// persistence still goes through the store and run-state transition helpers.
    run_state_snapshot: WorkflowRunState,
}

impl<'a, T: ReducerStore> NodeHandlerContext<'a, T> {
    pub(super) fn new(step: NodeStepContext<'a, T>) -> Self {
        let run_state_snapshot = WorkflowRunState::from_state(&step.workflow_run.state);
        Self {
            step,
            run_state_snapshot,
        }
    }

    /// typed, read-only state captured at the start of this reducer step.
    pub(super) fn run_state_snapshot(&self) -> &WorkflowRunState {
        &self.run_state_snapshot
    }
}

impl<'a, T: ReducerStore> Deref for NodeHandlerContext<'a, T> {
    type Target = NodeStepContext<'a, T>;

    fn deref(&self) -> &Self::Target {
        &self.step
    }
}

/// the processing contract every node kind must fulfill.
///
/// implementors return `KeepClaim` when the workflow must stay parked (e.g. a timer
/// that has not yet elapsed) and `Complete` in all other cases.
pub(super) trait NodeHandler<T: ReducerStore> {
    fn process<'a>(
        &'a self,
        ctx: &'a NodeHandlerContext<'a, T>,
    ) -> impl Future<Output = Result<ReadyNodeDisposition, SendableError>> + Send + 'a
    where
        T: 'a;
}

pub(super) fn complete(
    result: Result<(), SendableError>,
) -> Result<ReadyNodeDisposition, SendableError> {
    result?;
    Ok(ReadyNodeDisposition::Complete)
}
