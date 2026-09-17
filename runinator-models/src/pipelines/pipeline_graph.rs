#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineGraph {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub members: Vec<PipelineMember>,
    #[serde(default)]
    pub links: Vec<PipelineLink>,
    #[serde(default)]
    pub joins: BTreeMap<String, PipelineJoin>,
}

impl PipelineGraph {
    pub fn is_current(&self) -> bool {
        self.version == PIPELINE_GRAPH_VERSION
    }
}
