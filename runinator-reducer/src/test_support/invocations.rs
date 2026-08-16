//! the `InvocationStore` half of [`FakeStore`].
//!
//! split out because it is a self-contained domain with its own state, and because the invariant it
//! has to reproduce is specific: `suspend_invocation` must be atomic and idempotent by
//! `(invocation, sequence)`. a fake that appended blindly would let a reducer test pass while the
//! real store deduped — which is the one behavior most worth testing here.

use super::*;

use runinator_models::invocation::{
    InvocationContinuation, NewInvocationCall, WorkflowInvocation, WorkflowInvocationCall,
};
use runinator_store::roles::InvocationStore;

/// the invocation rows the fake remembers.
#[derive(Default)]
pub(super) struct InvocationState {
    pub invocations: Vec<WorkflowInvocation>,
    pub calls: Vec<WorkflowInvocationCall>,
}

impl FakeStore {
    /// every call recorded so far, in insertion order. lets a test assert that a program yielded
    /// exactly the calls it should have, and no more.
    pub fn recorded_invocation_calls(&self) -> Vec<WorkflowInvocationCall> {
        self.state
            .lock()
            .expect("state")
            .invocations_state
            .calls
            .clone()
    }

    /// the invocation attached to a node run, if a handler created one.
    pub fn invocation_for(&self, workflow_node_run_id: Uuid) -> Option<WorkflowInvocation> {
        self.state
            .lock()
            .expect("state")
            .invocations_state
            .invocations
            .iter()
            .find(|item| item.workflow_node_run_id == workflow_node_run_id)
            .cloned()
    }
}

impl InvocationStore for FakeStore {
    async fn create_invocation(
        &self,
        workflow_run_id: Uuid,
        workflow_node_run_id: Uuid,
        cursor_id: Option<Uuid>,
        node_id: &str,
        module_version: u32,
        continuation: &InvocationContinuation,
    ) -> Result<WorkflowInvocation, SendableError> {
        let now = Utc::now().timestamp();
        let record = WorkflowInvocation {
            id: Uuid::now_v7(),
            workflow_run_id,
            workflow_node_run_id,
            cursor_id,
            node_id: node_id.to_string(),
            module_version,
            continuation: continuation.clone(),
            status: WorkflowStatus::Running,
            output: None,
            message: None,
            created_at: now,
            updated_at: now,
            finished_at: None,
        };
        self.state
            .lock()
            .expect("state")
            .invocations_state
            .invocations
            .push(record.clone());
        Ok(record)
    }

    async fn fetch_invocation(
        &self,
        invocation_id: Uuid,
    ) -> Result<Option<WorkflowInvocation>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .invocations_state
            .invocations
            .iter()
            .find(|item| item.id == invocation_id)
            .cloned())
    }

    async fn fetch_invocation_for_node_run(
        &self,
        workflow_node_run_id: Uuid,
    ) -> Result<Option<WorkflowInvocation>, SendableError> {
        Ok(self.invocation_for(workflow_node_run_id))
    }

    async fn fetch_invocations_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<WorkflowInvocation>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .invocations_state
            .invocations
            .iter()
            .filter(|item| item.workflow_run_id == workflow_run_id)
            .cloned()
            .collect())
    }

    async fn suspend_invocation(
        &self,
        continuation: &InvocationContinuation,
        call: NewInvocationCall,
        command: ActionCommand,
    ) -> Result<WorkflowInvocationCall, SendableError> {
        let mut state = self.state.lock().expect("state");
        // idempotent by (invocation, sequence), matching the sql unique index. a duplicate drive
        // returns the recorded call and enqueues nothing.
        if let Some(existing) =
            state.invocations_state.calls.iter().find(|item| {
                item.invocation_id == call.invocation_id && item.sequence == call.sequence
            })
        {
            return Ok(existing.clone());
        }

        let now = Utc::now().timestamp();
        if let Some(invocation) = state
            .invocations_state
            .invocations
            .iter_mut()
            .find(|item| item.id == call.invocation_id)
        {
            invocation.continuation = continuation.clone();
            invocation.updated_at = now;
        }

        let record = WorkflowInvocationCall {
            id: Uuid::now_v7(),
            invocation_id: call.invocation_id,
            workflow_run_id: call.workflow_run_id,
            sequence: call.sequence,
            target: call.target,
            arguments: call.arguments,
            policy: call.policy,
            attempt: 0,
            status: WorkflowStatus::Running,
            result: None,
            message: None,
            idempotency_key: call.idempotency_key,
            deadline_at: call.deadline_at,
            current_executor_replica_id: None,
            last_executor_replica_id: None,
            executor_claimed_at: None,
            executor_released_at: None,
            created_at: now,
            started_at: Some(now),
            finished_at: None,
        };
        state.invocations_state.calls.push(record.clone());
        state.dispatches.push(ActionDispatchRecord {
            id: Uuid::now_v7(),
            dedupe_key: record.dispatch_key(),
            command,
            attempts: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            claimed_by: None,
            claimed_until: None,
            published_at: None,
            last_error: None,
        });
        Ok(record)
    }

    async fn update_invocation_continuation(
        &self,
        invocation_id: Uuid,
        continuation: &InvocationContinuation,
    ) -> Result<(), SendableError> {
        let mut state = self.state.lock().expect("state");
        if let Some(invocation) = state
            .invocations_state
            .invocations
            .iter_mut()
            .find(|item| item.id == invocation_id)
        {
            invocation.continuation = continuation.clone();
            invocation.updated_at = Utc::now().timestamp();
        }
        Ok(())
    }

    async fn settle_invocation(
        &self,
        invocation_id: Uuid,
        status: WorkflowStatus,
        output: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let mut state = self.state.lock().expect("state");
        if let Some(invocation) = state
            .invocations_state
            .invocations
            .iter_mut()
            .find(|item| item.id == invocation_id)
        {
            invocation.status = status;
            invocation.output = output;
            invocation.message = message;
            invocation.updated_at = Utc::now().timestamp();
            invocation.finished_at = status.is_terminal().then(|| Utc::now().timestamp());
        }
        Ok(())
    }

    async fn fetch_invocation_call(
        &self,
        call_id: Uuid,
    ) -> Result<Option<WorkflowInvocationCall>, SendableError> {
        Ok(self
            .state
            .lock()
            .expect("state")
            .invocations_state
            .calls
            .iter()
            .find(|item| item.id == call_id)
            .cloned())
    }

    async fn fetch_invocation_calls(
        &self,
        invocation_id: Uuid,
    ) -> Result<Vec<WorkflowInvocationCall>, SendableError> {
        let mut calls = self
            .state
            .lock()
            .expect("state")
            .invocations_state
            .calls
            .iter()
            .filter(|item| item.invocation_id == invocation_id)
            .cloned()
            .collect::<Vec<_>>();
        calls.sort_by_key(|item| item.sequence);
        Ok(calls)
    }

    async fn fetch_pending_invocation_call(
        &self,
        invocation_id: Uuid,
    ) -> Result<Option<WorkflowInvocationCall>, SendableError> {
        let mut open = self
            .state
            .lock()
            .expect("state")
            .invocations_state
            .calls
            .iter()
            .filter(|item| item.invocation_id == invocation_id && !item.status.is_terminal())
            .cloned()
            .collect::<Vec<_>>();
        open.sort_by_key(|item| item.sequence);
        Ok(open.pop())
    }

    async fn settle_invocation_call(
        &self,
        call_id: Uuid,
        attempt: i64,
        status: WorkflowStatus,
        result: Option<Value>,
        message: Option<String>,
    ) -> Result<bool, SendableError> {
        let mut state = self.state.lock().expect("state");
        let Some(call) = state
            .invocations_state
            .calls
            .iter_mut()
            .find(|item| item.id == call_id)
        else {
            return Ok(false);
        };
        // the same guard the sql update carries: a superseded attempt, or a duplicate of one already
        // applied, must not overwrite the settled outcome.
        if call.attempt != attempt || call.status.is_terminal() {
            return Ok(false);
        }
        call.status = status;
        call.result = result;
        call.message = message;
        call.finished_at = Some(Utc::now().timestamp());
        call.executor_released_at = call.finished_at;
        Ok(true)
    }

    async fn retry_invocation_call(
        &self,
        call_id: Uuid,
        deadline_at: Option<i64>,
        command: ActionCommand,
    ) -> Result<WorkflowInvocationCall, SendableError> {
        let mut state = self.state.lock().expect("state");
        let Some(call) = state
            .invocations_state
            .calls
            .iter_mut()
            .find(|item| item.id == call_id)
        else {
            return Err("unknown invocation call".into());
        };
        call.attempt += 1;
        call.status = WorkflowStatus::Running;
        call.result = None;
        call.message = None;
        call.deadline_at = deadline_at;
        call.started_at = Some(Utc::now().timestamp());
        call.finished_at = None;
        call.current_executor_replica_id = None;
        call.executor_claimed_at = None;
        call.executor_released_at = None;
        let record = call.clone();
        state.dispatches.push(ActionDispatchRecord {
            id: Uuid::now_v7(),
            dedupe_key: record.dispatch_key(),
            command,
            attempts: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            claimed_by: None,
            claimed_until: None,
            published_at: None,
            last_error: None,
        });
        Ok(record)
    }

    async fn set_invocation_call_executor(
        &self,
        call_id: Uuid,
        replica_id: Option<Uuid>,
    ) -> Result<(), SendableError> {
        let mut state = self.state.lock().expect("state");
        if let Some(call) = state
            .invocations_state
            .calls
            .iter_mut()
            .find(|item| item.id == call_id)
        {
            let now = Utc::now().timestamp();
            match replica_id {
                Some(replica_id) => {
                    call.current_executor_replica_id = Some(replica_id);
                    call.last_executor_replica_id = Some(replica_id);
                    call.executor_claimed_at = Some(now);
                    call.executor_released_at = None;
                }
                None => {
                    call.current_executor_replica_id = None;
                    call.executor_released_at = Some(now);
                }
            }
        }
        Ok(())
    }

    async fn cancel_invocation_calls_for_run(
        &self,
        workflow_run_id: Uuid,
        message: &str,
    ) -> Result<u64, SendableError> {
        let mut state = self.state.lock().expect("state");
        let now = Utc::now().timestamp();
        let mut cancelled = 0;
        for call in state.invocations_state.calls.iter_mut() {
            if call.workflow_run_id != workflow_run_id || call.status.is_terminal() {
                continue;
            }
            call.status = WorkflowStatus::Canceled;
            call.message = Some(message.to_string());
            call.finished_at = Some(now);
            call.executor_released_at = Some(now);
            cancelled += 1;
        }
        Ok(cancelled)
    }
}
