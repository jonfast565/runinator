#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterInspection {
    pub adapter_id: Uuid,
    pub mode: ExternalIngressGateMode,
}
