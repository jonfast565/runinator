//! the persisted shape of a running invocation and the durable calls it makes.
//!
//! these are the rows, not the ir: [`InvocationContinuation`] is the frozen program state and lives
//! inside [`WorkflowInvocation::continuation`], while a [`WorkflowInvocationCall`] is one durable
//! call the vm yielded on. the split matters because they have different lifetimes — a continuation
//! is rewritten on every yield and resume, and a call row is written once and settled once.
//!
//! status uses [`WorkflowStatus`] rather than a private vocabulary. an invocation and its calls sit
//! in the same lifecycle every other unit of work does, and a second set of names would have to be
//! mapped at every boundary that already knows this one.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{CallPolicy, CallableTarget, InvocationContinuation};
use crate::value::Value;
use crate::workflows::WorkflowStatus;

/// one authored node's program, frozen between the durable calls it makes.
///
/// the module is deliberately absent: it lives in the run's workflow snapshot, which already
/// insulates an in-flight run from a redefinition. only the version is stored, so a resume can
/// refuse a continuation the current module would misread rather than silently running the wrong
/// instructions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInvocation {
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    /// the node run this invocation *is*. one node run spans every call, which is the whole point:
    /// retries, logs and artifacts stay attributed to the authored node.
    pub workflow_node_run_id: Uuid,
    /// the thread of control that owns it. a fan-out can put two invocations on one node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<Uuid>,
    pub node_id: String,
    pub module_version: u32,
    pub continuation: InvocationContinuation,
    pub status: WorkflowStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

/// one durable call an invocation yielded on.
///
/// `sequence` is assigned by the vm's own call counter, not by insertion order, which is what makes
/// it idempotent: a duplicated drive re-reaches the same call with the same sequence and collides
/// with the unique index instead of dispatching twice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInvocationCall {
    pub id: Uuid,
    pub invocation_id: Uuid,
    pub workflow_run_id: Uuid,
    pub sequence: i64,
    pub target: CallableTarget,
    #[serde(default)]
    pub arguments: Vec<Value>,
    #[serde(default)]
    pub policy: CallPolicy,
    #[serde(default)]
    pub attempt: i64,
    pub status: WorkflowStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_claimed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_released_at: Option<i64>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

impl WorkflowInvocationCall {
    /// the dedupe key this call's dispatch is enqueued under.
    ///
    /// scoped to the attempt for the same reason a node run's is: outbox rows persist after publish,
    /// so a retry reusing the call's key would collide with the already-published row and never
    /// dispatch again.
    pub fn dispatch_key(&self) -> String {
        format!("workflow-invocation-call:{}:{}", self.id, self.attempt)
    }
}

/// what a caller supplies to record a yielded call.
#[derive(Debug, Clone, PartialEq)]
pub struct NewInvocationCall {
    /// the id this call will have.
    ///
    /// supplied by the caller rather than assigned by the store, because the dispatch that carries
    /// `invocation_call_id` has to name *this* row. a store-assigned id would leave the command
    /// pointing at a call that does not exist, and the worker's result would settle nothing — the
    /// invocation would sit parked until its node timeout, with no error anywhere to say why.
    pub id: Uuid,
    pub invocation_id: Uuid,
    pub workflow_run_id: Uuid,
    pub sequence: i64,
    pub target: CallableTarget,
    pub arguments: Vec<Value>,
    pub policy: CallPolicy,
    pub idempotency_key: Option<String>,
    pub deadline_at: Option<i64>,
}
