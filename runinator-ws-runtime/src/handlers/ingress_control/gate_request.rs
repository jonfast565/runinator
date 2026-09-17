#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct GateRequest {
    pub(super) mode: ExternalIngressGateMode,
}

impl Validate for GateRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
