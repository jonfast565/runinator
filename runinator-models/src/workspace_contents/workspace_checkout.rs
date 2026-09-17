#[allow(unused_imports)]
use super::*;

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
