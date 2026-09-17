#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowCompensationFrame {
    #[serde(default)]
    pub pending: Vec<WorkflowEffectRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<WorkflowEffectRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<usize>,
}
