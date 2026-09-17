#[allow(unused_imports)]
use super::*;

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
