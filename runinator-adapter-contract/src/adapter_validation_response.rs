#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdapterValidationResponse {
    #[serde(default)]
    pub issues: Vec<AdapterValidationIssue>,
}

impl AdapterValidationResponse {
    pub fn is_valid(&self) -> bool {
        !self
            .issues
            .iter()
            .any(|issue| issue.severity == AdapterValidationSeverity::Error)
    }
}
