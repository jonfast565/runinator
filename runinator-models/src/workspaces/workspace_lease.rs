#[allow(unused_imports)]
use super::*;

/// Durable control-plane record for one admission-scoped, worker-local workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceLease {
    pub id: Uuid,
    pub admission_id: Uuid,
    pub generation: i64,
    /// Caller-defined logical slot within an admission generation.
    pub scope: String,
    pub attempt: i64,
    pub worker_instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_replica_id: Option<Uuid>,
    /// Opaque key interpreted only by the selected worker/runtime.
    pub local_key: String,
    /// Snapshotted placement constraints used for worker selection.
    #[serde(default)]
    pub requirements: Value,
    pub status: WorkspaceStatus,
    pub version: i64,
    pub leased_until: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_since: Option<DateTime<Utc>>,
    /// Set only after the idempotent workspace-abandoned inbox event is durable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandonment_notified_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub evidence: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceLease {
    pub fn affinity(&self) -> WorkspaceAffinity {
        WorkspaceAffinity {
            workspace_id: self.id,
            worker_instance_id: self.worker_instance_id.clone(),
            local_key: self.local_key.clone(),
            attempt: self.attempt,
            version: self.version,
        }
    }
}
