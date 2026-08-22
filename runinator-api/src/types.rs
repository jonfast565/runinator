use runinator_models::value::Value;
use runinator_models::{
    runs::{NewRunArtifact, NewRunChunk, RunStatus},
    workflows::WorkflowStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatusPayload {
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeRunStatusPayload {
    pub status: WorkflowStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub type RunChunkPayload = NewRunChunk;
pub type RunArtifactPayload = NewRunArtifact;

/// what `POST /artifacts/content` returns: where the bytes landed and what they hashed to.
///
/// The caller records the `uri` on the artifact it is already reporting.
/// The `sha256` value verifies the upload without reading the object again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactContentResponse {
    pub uri: String,
    pub size_bytes: i64,
    pub sha256: String,
}
