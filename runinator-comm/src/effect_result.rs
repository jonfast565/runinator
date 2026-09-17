#[allow(unused_imports)]
use super::*;

/// A worker or infrastructure host's terminal or streaming report for one VM effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_commit: Option<Box<runinator_models::workspaces::WorkspaceCommit>>,
    pub version: u32,
    pub event_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub attempt: u32,
    /// Provider-normalized AI usage for this terminal attempt. This is accounting metadata and is
    /// deliberately separate from the workflow-visible output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_usage: Option<runinator_models::ai_usage::AiUsage>,
    pub kind: EffectResultKind,
    pub timestamp: DateTime<Utc>,
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    /// Copied from the originating command when this result settles a durable notification
    /// delivery instead of a workflow-owned effect receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivery_id: Option<Uuid>,
}

impl EffectResult {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_EFFECT_PROTOCOL_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_effect_protocol_version(self.version)
    }
}

impl EffectResult {
    pub fn status(
        command: &EffectCommand,
        status: WorkflowEffectStatus,
        output: Option<Value>,
        message: Option<String>,
    ) -> Self {
        Self {
            workspace_commit: None,
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            event_id: Uuid::now_v7(),
            effect_id: command.effect_id,
            workflow_run_id: command.workflow_run_id,
            continuation_id: command.continuation_id,
            attempt: command.attempt,
            ai_usage: None,
            kind: EffectResultKind::Status {
                status,
                output,
                message,
            },
            timestamp: Utc::now(),
            trace_id: command.trace_id,
            notification_delivery_id: command.notification_delivery_id,
        }
    }

    /// Announce that `executor_replica_id` has taken this attempt.
    pub fn claimed(command: &EffectCommand, executor_replica_id: Uuid) -> Self {
        Self {
            workspace_commit: None,
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            event_id: Uuid::now_v7(),
            effect_id: command.effect_id,
            workflow_run_id: command.workflow_run_id,
            continuation_id: command.continuation_id,
            attempt: command.attempt,
            ai_usage: None,
            kind: EffectResultKind::Claimed {
                executor_replica_id,
            },
            timestamp: Utc::now(),
            trace_id: command.trace_id,
            notification_delivery_id: command.notification_delivery_id,
        }
    }
}
