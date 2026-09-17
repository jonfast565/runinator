#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateOrgMemberRequest {
    pub role: OrgRole,
}

impl Validate for UpdateOrgMemberRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
