#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SessionRequest {
    pub(super) scope: ScopeRef,
    pub(super) mode: BrokerIngressSessionMode,
}

impl Validate for SessionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
