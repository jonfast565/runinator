#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineLink {
    pub id: Uuid,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub on: PipelineLinkSelector,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub parameters: Value,
}
