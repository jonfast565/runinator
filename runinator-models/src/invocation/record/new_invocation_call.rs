#[allow(unused_imports)]
use super::*;

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
