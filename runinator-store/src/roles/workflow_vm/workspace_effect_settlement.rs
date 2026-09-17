#[allow(unused_imports)]
use super::*;

pub struct WorkspaceEffectSettlement {
    pub effect_id: Uuid,
    pub attempt: u32,
    pub status: WorkflowEffectStatus,
    pub output: Option<Value>,
    pub message: Option<String>,
    pub settled_at: DateTime<Utc>,
    pub workspace: Option<runinator_models::workspaces::WorkspaceCommit>,
}
