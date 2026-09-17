#[allow(unused_imports)]
use super::*;

/// Generic durable work published by the workflow VM host.
///
/// This is not coupled to a node-run record. The effect id identifies
/// the one persisted receipt that a result may settle, and the continuation id identifies exactly
/// which suspended VM branch becomes runnable afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectCommand {
    pub version: u32,
    pub command_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub attempt: u32,
    pub request: WorkflowEffectRequest,
    /// Selects the class of host allowed to claim this command. Provider workers and the
    /// infrastructure coordinator share the effect protocol, but must never compete for the same
    /// request kind.
    pub executor: EffectExecutor,
    #[serde(default)]
    pub target: ActionTarget,
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub trace_context: std::collections::HashMap<String, String>,
    pub idempotency_key: String,
    /// Set for an engine-owned notification delivery. Such a command shares the provider-effect
    /// transport and worker executor, but is settled against `notification_deliveries`, never a
    /// workflow effect receipt or continuation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivery_id: Option<Uuid>,
}

impl EffectCommand {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_EFFECT_PROTOCOL_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_effect_protocol_version(self.version)
    }
}
