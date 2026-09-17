#[allow(unused_imports)]
use super::*;

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
