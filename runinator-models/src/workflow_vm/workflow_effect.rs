#[allow(unused_imports)]
use super::*;

/// The canonical durable receipt for a yielded effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowEffect {
    pub version: u32,
    pub id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub sequence: u64,
    pub attempt: u32,
    /// Source-map projection populated by the operator API. It is not stored with the effect
    /// receipt, because the pinned module is the source of truth for that relationship.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    /// Derived from the effect request by the backend rather than persisted independently.
    #[serde(default)]
    pub timeline_category: WorkflowTimelineCategory,
    pub request: WorkflowEffectRequest,
    pub status: WorkflowEffectStatus,
    /// Replica currently executing this attempt, set when a host claims the delivery and cleared
    /// when the effect settles. This is the VM's executor lease: it replaces the node-run executor
    /// columns, so replica load and dead-worker recovery read effects rather than node runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_executor_replica_id: Option<Uuid>,
    /// Last replica to have claimed this effect, retained after settlement for attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Unix seconds. Immutable receipt creation time, independent of broker publication.
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<i64>,
}

impl WorkflowEffect {
    pub fn idempotency_key(&self) -> String {
        format!(
            "workflow-effect:{}:{}:{}",
            self.continuation_id, self.sequence, self.attempt
        )
    }

    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_EFFECT_PROTOCOL_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_effect_protocol_version(self.version)
    }
}
