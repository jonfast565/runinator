#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchOrgRequest {
    pub org_id: Uuid,
}

impl Validate for SwitchOrgRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
