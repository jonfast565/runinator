#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceReaderLease {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub version: i64,
    pub expires_at: DateTime<Utc>,
}
