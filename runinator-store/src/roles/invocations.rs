//! resumable invocations and the durable calls they yield on.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only needs
//! this slice of the store.
//!
//! the operations are shaped around one invariant: a program that yields must persist its
//! continuation, record the call, and enqueue the dispatch *atomically*. if the continuation landed
//! without the call, the invocation would wait forever on a call nobody made; if the call landed
//! without the continuation, its result would resume a program that never suspended. that is why
//! [`InvocationStore::suspend_invocation`] is one operation rather than three composable ones.

use std::future::Future;

use uuid::Uuid;

use runinator_comm::ActionCommand;
use runinator_models::{
    errors::SendableError,
    invocation::{
        InvocationContinuation, NewInvocationCall, WorkflowInvocation, WorkflowInvocationCall,
    },
    value::Value,
    workflows::WorkflowStatus,
};

/// Persistence for the resumable invocation runtime.
pub trait InvocationStore: Send + Sync + 'static {
    /// Create the invocation backing a node run, positioned at the start of its module.
    fn create_invocation(
        &self,
        workflow_run_id: Uuid,
        workflow_node_run_id: Uuid,
        cursor_id: Option<Uuid>,
        node_id: &str,
        module_version: u32,
        continuation: &InvocationContinuation,
    ) -> impl Future<Output = Result<WorkflowInvocation, SendableError>> + Send;

    /// Fetch the invocation for a node run, if it has one.
    fn fetch_invocation_for_node_run(
        &self,
        workflow_node_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowInvocation>, SendableError>> + Send;

    /// Fetch one invocation by id.
    fn fetch_invocation(
        &self,
        invocation_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowInvocation>, SendableError>> + Send;

    /// Fetch every invocation belonging to a run, oldest first.
    fn fetch_invocations_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowInvocation>, SendableError>> + Send;

    /// Suspend an invocation on a durable call: store the continuation, record the call, and enqueue
    /// its dispatch — all in one transaction.
    ///
    /// Returns the recorded call. A call whose `(invocation, sequence)` already exists is returned
    /// unchanged and enqueues nothing, which is what makes a duplicated drive a no-op.
    fn suspend_invocation(
        &self,
        continuation: &InvocationContinuation,
        call: NewInvocationCall,
        command: ActionCommand,
    ) -> impl Future<Output = Result<WorkflowInvocationCall, SendableError>> + Send;

    /// Store a continuation without yielding, after an in-process step made progress.
    fn update_invocation_continuation(
        &self,
        invocation_id: Uuid,
        continuation: &InvocationContinuation,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Settle an invocation.
    fn settle_invocation(
        &self,
        invocation_id: Uuid,
        status: WorkflowStatus,
        output: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch one call by id.
    fn fetch_invocation_call(
        &self,
        call_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowInvocationCall>, SendableError>> + Send;

    /// Fetch every call an invocation has made, in sequence order.
    fn fetch_invocation_calls(
        &self,
        invocation_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowInvocationCall>, SendableError>> + Send;

    /// Fetch the call an invocation is currently parked on, if any.
    fn fetch_pending_invocation_call(
        &self,
        invocation_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowInvocationCall>, SendableError>> + Send;

    /// Settle a call with its terminal outcome.
    ///
    /// Returns `false` when the call was already terminal, which is how a late or duplicated result
    /// is discarded rather than applied twice.
    fn settle_invocation_call(
        &self,
        call_id: Uuid,
        attempt: i64,
        status: WorkflowStatus,
        result: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Start the next attempt of a call and enqueue its dispatch, in one transaction.
    fn retry_invocation_call(
        &self,
        call_id: Uuid,
        deadline_at: Option<i64>,
        command: ActionCommand,
    ) -> impl Future<Output = Result<WorkflowInvocationCall, SendableError>> + Send;

    /// Claim a call for an executing replica, or release the claim when `replica_id` is `None`.
    fn set_invocation_call_executor(
        &self,
        call_id: Uuid,
        replica_id: Option<Uuid>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Cancel every non-terminal call under a run, for a run-level cancellation.
    fn cancel_invocation_calls_for_run(
        &self,
        workflow_run_id: Uuid,
        message: &str,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;
}
