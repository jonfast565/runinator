use runinator_models::value::Value;
use serde::{Deserialize, Serialize};

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

/// Stable response from a workflow or pipeline ingress admission. Kept in the client crate so
/// CLI/MCP callers can start a managed mission without importing web-handler types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressResponse {
    pub admission_id: String,
    pub generation: i64,
    pub disposition: String,
    pub duplicate: bool,
    pub queue_position: Option<i64>,
    pub workflow_run_id: Option<String>,
    pub pipeline_run_id: Option<String>,
    pub orchestration_binding_id: Option<String>,
    pub message: String,
}

/// Immutable envelope submitted to a pipeline's ingress policy. Keeping the event identity,
/// correlation, payload, and provenance together prevents ingress callers from accidentally
/// mismatching the durable audit fields.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineIngressRequest {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    pub payload: Value,
    pub provenance: Value,
}
