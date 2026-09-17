#[allow(unused_imports)]
use super::*;

/// a mocked node outcome in a test spec.
#[derive(Debug, Clone, Deserialize)]
pub struct MockSpec {
    #[serde(default)]
    pub output: Value,
    #[serde(default = "default_mock_status")]
    pub status: WorkflowStatus,
}
