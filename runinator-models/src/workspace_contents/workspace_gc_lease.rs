#[allow(unused_imports)]
use super::*;

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
