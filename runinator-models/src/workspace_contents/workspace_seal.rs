#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSeal {
    pub revision_id: String,
}

impl crate::validation::Validate for WorkspaceSeal {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        if !runinator_hash::is_valid_hex(&self.revision_id) {
            return Err(crate::validation::ValidationError::new(
                "revision_id",
                "must be a 64-digit object identity",
            ));
        }
        Ok(())
    }
}
