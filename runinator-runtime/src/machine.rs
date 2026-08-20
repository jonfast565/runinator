//! Continuation-driven interpreter for a validated workflow graph.

use runinator_models::{
    errors::SendableError, orchestration::ReadyNodeRecord, workflows::WorkflowNodeKind,
};
use runinator_store::RuntimeStore;
use uuid::Uuid;

use crate::orchestration;

/// Observable result of one machine drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveOutcome {
    /// No live addressed cursor remained to execute.
    Idle,
    /// The cursor parked on a durable runtime effect.
    Suspended(Suspension),
    /// The cursor retired, possibly completing its run.
    Retired,
    /// Debugging paused the cursor or run.
    Paused,
    /// The safety budget blocked a cursor that failed to settle.
    Blocked,
    /// The current ready claim must remain held while the cursor waits for an immediate condition.
    KeepClaim,
}

/// Why a cursor yielded control back to the durable host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suspension {
    ProviderCall,
    Timer,
    Approval,
    Signal,
    Input,
    ChildRun,
    Coordination,
    Event,
    External,
}

impl Suspension {
    pub(crate) fn for_node(kind: &WorkflowNodeKind) -> Self {
        match kind {
            WorkflowNodeKind::Action | WorkflowNodeKind::Invocation => Self::ProviderCall,
            WorkflowNodeKind::Wait | WorkflowNodeKind::Debounce | WorkflowNodeKind::Cooldown => {
                Self::Timer
            }
            WorkflowNodeKind::Approval | WorkflowNodeKind::Gate => Self::Approval,
            WorkflowNodeKind::Signal => Self::Signal,
            WorkflowNodeKind::Input => Self::Input,
            WorkflowNodeKind::Subflow | WorkflowNodeKind::AwaitRun => Self::ChildRun,
            WorkflowNodeKind::Mutex
            | WorkflowNodeKind::Throttle
            | WorkflowNodeKind::Collect
            | WorkflowNodeKind::Barrier
            | WorkflowNodeKind::CircuitBreaker => Self::Coordination,
            WorkflowNodeKind::EventSource => Self::Event,
            _ => Self::External,
        }
    }
}

/// Runtime-native request to advance one durable graph cursor.
///
/// Queue claims are an engine concern. The interpreter needs only the run, the addressed cursor,
/// and a node hint for legacy wakes that predate cursor addressing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveRequest {
    pub workflow_run_id: Uuid,
    pub cursor_id: Option<Uuid>,
    pub node_hint: Option<String>,
}

impl DriveRequest {
    pub fn cursor(workflow_run_id: Uuid, cursor_id: Uuid) -> Self {
        Self {
            workflow_run_id,
            cursor_id: Some(cursor_id),
            node_hint: None,
        }
    }

    pub fn from_ready(ready: &ReadyNodeRecord) -> Self {
        Self {
            workflow_run_id: ready.workflow_run_id,
            cursor_id: ready.cursor_id,
            node_hint: Some(ready.node_id.clone()),
        }
    }
}

/// One interpreter over graph cursors and nested invocation continuations.
pub struct WorkflowMachine<'a, S: RuntimeStore> {
    store: &'a S,
}

impl<'a, S: RuntimeStore> WorkflowMachine<'a, S> {
    pub fn new(store: &'a S) -> Self {
        Self { store }
    }

    /// Drive an addressed durable cursor using the graph snapshot stored with its run.
    ///
    /// Scheduler integrations should prefer [`drive_ready`](Self::drive_ready), which preserves
    /// the identity of the claimed ready record. This form is useful to resume a cursor from an
    /// external event without manufacturing queue state in the caller.
    pub async fn drive(
        &self,
        workflow_run_id: Uuid,
        cursor_id: Uuid,
    ) -> Result<DriveOutcome, SendableError> {
        let run = self
            .store
            .fetch_workflow_run(workflow_run_id)
            .await?
            .ok_or_else(|| crate::errors::WORKFLOW_NOT_FOUND.error(workflow_run_id))?;
        match run.execution_state.cursor(cursor_id) {
            Some(_) => {}
            None => return Ok(DriveOutcome::Idle),
        }
        self.drive_request(&DriveRequest::cursor(workflow_run_id, cursor_id))
            .await
    }

    /// Drive from a scheduler-owned ready record.
    pub async fn drive_ready(
        &self,
        ready: &ReadyNodeRecord,
    ) -> Result<DriveOutcome, SendableError> {
        self.drive_request(&DriveRequest::from_ready(ready)).await
    }

    /// Drive a runtime-native cursor request.
    pub async fn drive_request(
        &self,
        request: &DriveRequest,
    ) -> Result<DriveOutcome, SendableError> {
        orchestration::interpreter::drive_cursor(self.store, request).await
    }
}
