#[allow(unused_imports)]
use super::*;

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
