#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateGrantRequest {
    pub principal_type: PrincipalType,
    pub principal_id: Uuid,
    pub permission: Permission,
}

impl Validate for CreateGrantRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
