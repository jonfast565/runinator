//! Portable workspace contents and immutable snapshot references.

use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceReference {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    Read,
    #[default]
    Write,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceAttachment {
    #[serde(default)]
    pub follow_run: bool,
    #[serde(flatten)]
    pub reference: WorkspaceReference,
    #[serde(default)]
    pub access: WorkspaceAccess,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub results: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DurableWorkspace {
    pub id: Uuid,
    pub key: String,
    pub org_id: Option<Uuid>,
    pub head_version: i64,
    pub revision: i64,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub executable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub workspace_id: Uuid,
    pub version: i64,
    pub parent_version: i64,
    pub workflow_run_id: Uuid,
    pub effect_id: Uuid,
    pub attempt: u32,
    pub archive_uri: String,
    pub archive_sha256: String,
    pub compressed_bytes: u64,
    pub files: Vec<WorkspaceFile>,
    pub results: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceCheckout {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub workflow_run_id: Uuid,
    pub effect_id: Uuid,
    pub attempt: u32,
    pub base_version: i64,
    pub access: WorkspaceAccess,
    pub fence: i64,
    pub leased_until: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceCommit {
    pub checkout: WorkspaceCheckout,
    pub snapshot: WorkspaceSnapshot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceAcquire {
    pub workspace_id: Uuid,
    pub workflow_run_id: Uuid,
    pub effect_id: Uuid,
    pub attempt: u32,
    pub version: Option<i64>,
    pub access: WorkspaceAccess,
    pub now: DateTime<Utc>,
    pub leased_until: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkspaceAcquisition {
    Acquired { checkout: WorkspaceCheckout },
    Busy,
    Conflict,
    Missing,
}

/// Engine-resolved workspace input supplied only to the assigned worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceExecution {
    pub key: String,
    pub checkout: WorkspaceCheckout,
    pub snapshot: Option<WorkspaceSnapshot>,
    pub results: BTreeMap<String, Value>,
}

/// Caller-specific management projection; authorization is never persisted on the identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceView {
    #[serde(flatten)]
    pub workspace: DurableWorkspace,
    pub permission: crate::auth::Permission,
}
