#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct WorkflowRunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub name: Option<String>,
    /// File ids referenced by typed input parameters. Staged inputs are claimed for this run and
    /// library revisions are validated immediately before the VM is nudged.
    #[serde(default)]
    pub file_ids: Vec<Uuid>,
}

impl Validate for WorkflowRunRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("name", self.name.as_deref(), SHORT_TEXT_MAX)?;
        if self.file_ids.len() > 128 {
            return Err(ValidationError::new(
                "file_ids",
                "must contain at most 128 files",
            ));
        }
        Ok(())
    }
}
