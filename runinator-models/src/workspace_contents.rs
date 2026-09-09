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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub workspace_id: Uuid,
    pub version: i64,
    pub parent_version: i64,
    pub origin: WorkspaceOrigin,
    pub revision_id: String,
    pub usage: WorkspaceUsage,
    pub limits: WorkspaceLimits,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceOrigin {
    Workflow {
        workflow_run_id: Uuid,
        effect_id: Uuid,
        attempt: u32,
    },
    Import {
        transfer_id: Uuid,
        format: String,
    },
}
impl WorkspaceOrigin {
    pub fn effect_id(&self) -> Option<Uuid> {
        match self {
            Self::Workflow { effect_id, .. } => Some(*effect_id),
            Self::Import { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceCheckout {
    pub limits: WorkspaceLimits,
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
    pub receipt_id: Uuid,
}

/// Server-validated physical location of one logical object within a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceObjectLocation {
    pub kind: u8,
    pub raw_len: u64,
    pub id: String,
    pub pack: String,
    pub offset: u64,
    pub length: u64,
    pub member: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSeal {
    pub revision_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceReceipt {
    pub id: Uuid,
    pub checkout: WorkspaceCheckout,
    pub snapshot: WorkspaceSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub kind: String,
    pub inode_number: u64,
    pub content_id: String,
    pub size_bytes: u64,
    pub executable: bool,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDirectory {
    pub revision_id: String,
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceDownload {
    pub transfer_id: Option<Uuid>,
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub version: i64,
    pub path: Option<String>,
    pub result: bool,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceDownloadRequest {
    #[serde(default)]
    pub transfer_id: Option<Uuid>,
    pub path: Option<String>,
    #[serde(default)]
    pub result: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDifference {
    pub path: String,
    pub result: bool,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDiff {
    pub before_revision: String,
    pub after_revision: String,
    pub changes: Vec<WorkspaceDifference>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceGcLease {
    pub object_count: u64,
    pub workspace_id: Uuid,
    pub token: Uuid,
    pub fence: i64,
    pub revision: i64,
    pub expires_at: DateTime<Utc>,
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceReaderLease {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub version: i64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceAcquire {
    pub limits: WorkspaceLimits,
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

/// Durable transfer state; archive bytes reside in the shared object store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTransfer {
    #[serde(default)]
    pub filesystem: bool,
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub version: i64,
    pub importing: bool,
    pub state: String,
    pub bytes_processed: u64,
    pub error: Option<String>,
    pub limits: WorkspaceLimits,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(skip)]
    pub token: Uuid,
    #[serde(skip)]
    pub archive_uri: Option<String>,
}

impl crate::validation::Validate for WorkspaceSeal {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        if self.revision_id.len() != 64
            || !self
                .revision_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(crate::validation::ValidationError::new(
                "revision_id",
                "must be a 64-digit object identity",
            ));
        }
        Ok(())
    }
}
impl crate::validation::Validate for WorkspaceDownloadRequest {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        if self.transfer_id.is_some() {
            if self.path.is_some() || self.result {
                return Err(crate::validation::ValidationError::new(
                    "transfer_id",
                    "cannot be combined with a file or result",
                ));
            }
        } else if !self
            .path
            .as_ref()
            .is_some_and(|path| !path.is_empty() && path.len() <= 4096 && !path.contains('\0'))
        {
            return Err(crate::validation::ValidationError::new(
                "path",
                "a path or result name of 1 to 4096 bytes is required",
            ));
        }
        Ok(())
    }
}
