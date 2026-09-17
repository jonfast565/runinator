#[allow(unused_imports)]
use super::*;

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
