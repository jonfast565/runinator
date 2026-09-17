#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceDownloadRequest {
    #[serde(default)]
    pub transfer_id: Option<Uuid>,
    pub path: Option<String>,
    #[serde(default)]
    pub result: bool,
}

impl crate::validation::Validate for WorkspaceDownloadRequest {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        if self.transfer_id.is_some() {
            if self.path.is_some() || self.result {
                return Err(crate::validation::ValidationError::new(
                    "transfer_id",
                    "cannot be combined with a file or result",
                ));
            }
        } else if !self
            .path
            .as_ref()
            .is_some_and(|path| !path.is_empty() && path.len() <= 4096 && !path.contains('\0'))
        {
            return Err(crate::validation::ValidationError::new(
                "path",
                "a path or result name of 1 to 4096 bytes is required",
            ));
        }
        Ok(())
    }
}
