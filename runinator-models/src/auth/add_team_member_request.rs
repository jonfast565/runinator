#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct AddTeamMemberRequest {
    pub user_id: Uuid,
    pub role: crate::rbac::TeamRole,
}

impl Validate for AddTeamMemberRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
