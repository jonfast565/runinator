#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRunEdgeState {
    pub link_id: Uuid,
    pub state: String,
}
