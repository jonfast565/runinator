#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SessionHeartbeatRequest {
    pub(super) scope: ScopeRef,
}

impl Validate for SessionHeartbeatRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
