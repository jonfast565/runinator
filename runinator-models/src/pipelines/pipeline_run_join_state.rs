#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunJoinState {
    pub target: String,
    pub mode: PipelineJoinMode,
    pub state: String,
    pub satisfied_inputs: usize,
    pub total_inputs: usize,
}
