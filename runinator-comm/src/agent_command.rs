#[allow(unused_imports)]
use super::*;

/// replica-scoped fleet-management command. unlike [`ControlCommand`], this is never associated
/// with a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommand {
    pub directive_id: Uuid,
    pub replica_id: Uuid,
    pub target: ActionTarget,
    pub kind: AgentDirectiveKind,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
