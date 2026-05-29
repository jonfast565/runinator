// the argument bundle and helper surface every node handler receives.
//
// `NodeContext` collapses the (api, broker, workflow_run, node, latest, node_runs) tuple that the
// old free functions threaded by hand, and exposes the shared persistence steps as methods so a
// handler never re-implements timeout checks, transitions, retries, or state plumbing. each method
// that settles or moves the run returns a `NodeOutcome` describing what happened, which the driver
// forwards to the lifecycle hooks.

use chrono::Utc;
use runinator_broker::Broker;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowNode, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};

use crate::api::WorkflowSchedulerApi;
use crate::context::runtime_context;
use crate::nodes::driver::{self, RetryDisposition};
use crate::nodes::handler::NodeOutcome;
use crate::nodes::run_state::RunState;

/// everything a node handler needs for one scheduling tick.
pub struct NodeContext<'a> {
    pub api: &'a dyn WorkflowSchedulerApi,
    /// present on the live scheduler path; `None` in direct unit-test calls that never enqueue.
    pub broker: Option<&'a dyn Broker>,
    pub workflow_run: &'a WorkflowRun,
    pub node: &'a WorkflowNode,
    /// the most recent run for this node, if any.
    pub latest: Option<&'a WorkflowNodeRun>,
    pub node_runs: &'a [WorkflowNodeRun],
}

impl<'a> NodeContext<'a> {
    pub fn new(
        api: &'a dyn WorkflowSchedulerApi,
        broker: Option<&'a dyn Broker>,
        workflow_run: &'a WorkflowRun,
        node: &'a WorkflowNode,
        latest: Option<&'a WorkflowNodeRun>,
        node_runs: &'a [WorkflowNodeRun],
    ) -> Self {
        Self {
            api,
            broker,
            workflow_run,
            node,
            latest,
            node_runs,
        }
    }

    // --- reads -------------------------------------------------------------

    /// the latest run for this node when it holds the given status.
    pub fn latest_with_status(&self, status: WorkflowStatus) -> Option<&WorkflowNodeRun> {
        self.latest.filter(|run| run.status == status)
    }

    /// build the `$input/$steps/$prev/$workflow` evaluation context for this run.
    pub fn runtime_context(&self) -> Value {
        runtime_context(self.workflow_run, self.node_runs)
    }

    /// typed builder over the run's `state` json for control-flow frame bookkeeping.
    pub fn run_state(&self) -> RunState {
        RunState::from_run(self.workflow_run)
    }

    /// true when the run started more than `node.timeout_seconds` ago.
    pub fn timed_out(&self, run: &WorkflowNodeRun) -> bool {
        let (Some(timeout), Some(started_at)) = (self.node.timeout_seconds, run.started_at) else {
            return false;
        };
        Utc::now() - started_at > chrono::Duration::seconds(timeout)
    }

    /// like `timed_out`, but measured from run creation. used by subflow waits which have no
    /// per-tick `started_at`.
    pub fn timed_out_since_created(&self, run: &WorkflowNodeRun) -> bool {
        let Some(timeout) = self.node.timeout_seconds else {
            return false;
        };
        Utc::now() - run.created_at > chrono::Duration::seconds(timeout)
    }

    // --- low-level passthroughs -------------------------------------------

    /// create a fresh run for this node, defaulting parameters to the node definition.
    pub async fn create_node_run(&self) -> Result<WorkflowNodeRun, SendableError> {
        self.create_node_run_with(self.node.parameters.clone())
            .await
    }

    pub async fn create_node_run_with(
        &self,
        parameters: Value,
    ) -> Result<WorkflowNodeRun, SendableError> {
        self.api
            .create_workflow_node_run(self.workflow_run.id, &self.node.id, parameters)
            .await
    }

    /// reuse the latest run, or create one if absent.
    pub async fn ensure_node_run(&self) -> Result<WorkflowNodeRun, SendableError> {
        driver::ensure_node_run(self.api, self.workflow_run, self.node, self.latest).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        self.api
            .update_workflow_node_run(
                node_run_id,
                status,
                attempt,
                parameters,
                output_json,
                state,
                reason,
                message,
            )
            .await
    }

    /// update this workflow run; the run id is implied by the context.
    pub async fn update_run(
        &self,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        self.api
            .update_workflow_run(self.workflow_run.id, status, active_node_id, state, message)
            .await
    }

    // --- flow steps (return a NodeOutcome for hooks) ----------------------

    /// settle `node_run` and advance along the node's transitions.
    pub async fn transition(
        &self,
        node_run: &WorkflowNodeRun,
        status: WorkflowStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<NodeOutcome, SendableError> {
        let target = driver::transition_from_node(
            self.api,
            self.workflow_run,
            self.node,
            node_run,
            status,
            output_json,
            message,
            self.node_runs,
        )
        .await?;
        Ok(NodeOutcome::Advanced { status, target })
    }

    /// requeue `node_run` if retries remain, otherwise settle and transition.
    pub async fn retry_or_transition(
        &self,
        node_run: &WorkflowNodeRun,
        status: WorkflowStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Result<NodeOutcome, SendableError> {
        let disposition = driver::retry_or_transition(
            self.api,
            self.workflow_run,
            self.node,
            node_run,
            status,
            output_json,
            message,
            self.node_runs,
        )
        .await?;
        Ok(match disposition {
            RetryDisposition::Retried => NodeOutcome::Retrying,
            RetryDisposition::Transitioned(target) => NodeOutcome::Advanced { status, target },
        })
    }

    /// time out the in-flight run with a node-specific message, retrying if attempts remain.
    pub async fn time_out(
        &self,
        node_run: &WorkflowNodeRun,
        message: &str,
    ) -> Result<NodeOutcome, SendableError> {
        self.retry_or_transition(
            node_run,
            WorkflowStatus::TimedOut,
            None,
            Some(message.into()),
        )
        .await
    }

    /// jump the workflow to `target`, optionally rewriting run state.
    pub async fn goto(
        &self,
        target: String,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<NodeOutcome, SendableError> {
        self.update_run(
            WorkflowStatus::Running,
            Some(target.clone()),
            state,
            message,
        )
        .await?;
        Ok(NodeOutcome::Advanced {
            status: WorkflowStatus::Running,
            target: Some(target),
        })
    }

    /// block the workflow with a message.
    pub async fn block(&self, message: &str) -> Result<NodeOutcome, SendableError> {
        driver::block_node(self.api, self.workflow_run, self.node, message).await?;
        Ok(NodeOutcome::Blocked)
    }
}
