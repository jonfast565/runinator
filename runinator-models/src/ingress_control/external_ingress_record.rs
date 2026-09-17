#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIngressRecord {
    #[serde(default)]
    pub adapter: Option<crate::adapter_control::AdapterOrigin>,
    #[serde(default)]
    pub caller_org_id: Option<Uuid>,
    pub id: Uuid,
    pub target: IngressTarget,
    pub owner_scope: ScopeRef,
    pub gate_mode: ExternalIngressGateMode,
    pub event: IngressEvent,
    pub state: IngressControlState,
    pub queue_position: Option<i64>,
    pub reviewed_by: Option<Uuid>,
    pub last_error: Option<String>,
    pub received_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}
