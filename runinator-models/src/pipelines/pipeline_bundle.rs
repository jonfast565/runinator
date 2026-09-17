#[allow(unused_imports)]
use super::*;

/// the compiled pipeline artifact carried in a pack zip as `pipelines.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineBundle {
    #[serde(default)]
    pub pipelines: Vec<PipelineSpec>,
}
