#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct AddOrgMemberRequest {
    pub user_id: Uuid,
    pub role: OrgRole,
}

impl Validate for AddOrgMemberRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
