#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIngressGate {
    pub target: IngressTarget,
    pub owner_scope: ScopeRef,
    pub mode: ExternalIngressGateMode,
    pub updated_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}
