#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub output_json: Option<Value>,
    #[serde(default)]
    pub chunks: Vec<NewRunChunk>,
    #[serde(default)]
    pub artifacts: Vec<NewRunArtifact>,
}

impl From<ProviderExecutionResponse> for TaskExecutionResult {
    fn from(response: ProviderExecutionResponse) -> Self {
        Self {
            message: response.message,
            output_json: response.output_json,
            chunks: response.chunks,
            artifacts: response.artifacts,
        }
    }
}
