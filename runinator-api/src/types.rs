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
