#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderExecutionResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default)]
    pub chunks: Vec<NewRunChunk>,
    #[serde(default)]
    pub artifacts: Vec<NewRunArtifact>,
}
