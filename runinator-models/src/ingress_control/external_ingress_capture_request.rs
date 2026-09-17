#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct ExternalIngressCaptureRequest {
    pub target: IngressTarget,
    pub owner_scope: ScopeRef,
    pub gate_mode: ExternalIngressGateMode,
    pub event: IngressEvent,
    pub adapter: Option<crate::adapter_control::AdapterOrigin>,
    pub now: DateTime<Utc>,
    pub capacity: i64,
}
