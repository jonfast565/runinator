use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;

/// Reserved worker label used to route filesystem-bound effects to a stable machine identity.
/// A worker runtime may mint a new replica id after restart; its instance id remains stable.
pub const WORKSPACE_INSTANCE_LABEL: &str = "runinator.instance";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Allocating,
    Active,
    Finalizing,
    Released,
    Abandoned,
}

impl WorkspaceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allocating => "allocating",
            Self::Active => "active",
            Self::Finalizing => "finalizing",
            Self::Released => "released",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Released | Self::Abandoned)
    }
}

impl TryFrom<&str> for WorkspaceStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "allocating" => Ok(Self::Allocating),
            "active" => Ok(Self::Active),
            "finalizing" => Ok(Self::Finalizing),
            "released" => Ok(Self::Released),
            "abandoned" => Ok(Self::Abandoned),
            other => Err(format!("unknown workspace status {other}")),
        }
    }
}

/// Immutable routing token copied into a workflow action. The version and attempt prevent a stale
/// continuation from silently reusing a superseded local workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAffinity {
    pub workspace_id: Uuid,
    pub worker_instance_id: String,
    /// Opaque relative key resolved beneath the selected worker's configured workspace root.
    #[serde(default)]
    pub local_key: String,
    pub attempt: i64,
    pub version: i64,
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewWorkspaceLease {
    pub id: Uuid,
    pub admission_id: Uuid,
    pub generation: i64,
    pub scope: String,
    pub attempt: i64,
    pub worker_instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_replica_id: Option<Uuid>,
    pub local_key: String,
    #[serde(default)]
    pub requirements: Value,
    pub leased_until: DateTime<Utc>,
}
