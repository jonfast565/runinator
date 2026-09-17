#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSeal {
    pub revision_id: String,
}

impl crate::validation::Validate for WorkspaceSeal {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        if self.revision_id.len() != 64
            || !self
                .revision_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(crate::validation::ValidationError::new(
                "revision_id",
                "must be a 64-digit object identity",
            ));
        }
        Ok(())
    }
}
