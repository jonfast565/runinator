#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineJoinSpec {
    pub target: String,
    pub mode: PipelineJoinMode,
    #[serde(default)]
    pub parameters: Value,
}
