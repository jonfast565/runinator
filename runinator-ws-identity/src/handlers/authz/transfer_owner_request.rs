#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct TransferOwnerRequest {
    pub owner: ScopeRef,
}

impl Validate for TransferOwnerRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
