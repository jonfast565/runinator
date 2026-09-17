#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterValidationIssue {
    pub path: String,
    pub code: String,
    pub message: String,
    pub severity: AdapterValidationSeverity,
}
