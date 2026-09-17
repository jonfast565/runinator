#[allow(unused_imports)]
use super::*;

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
