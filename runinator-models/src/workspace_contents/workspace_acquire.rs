#[allow(unused_imports)]
use super::*;

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
