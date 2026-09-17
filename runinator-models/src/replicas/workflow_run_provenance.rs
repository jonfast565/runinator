#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowRunProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<TriggerSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_type: Option<TriggerActorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_ip: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}
