#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct SetRoleRequest {
    pub role: Role,
}

impl Validate for SetRoleRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
