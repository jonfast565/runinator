#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineIngressResult {
    pub admission_id: Uuid,
    pub generation: i64,
    pub disposition: String,
    pub duplicate: bool,
    pub queue_position: Option<i64>,
    pub workflow_run_id: Option<Uuid>,
    pub pipeline_run_id: Option<Uuid>,
    pub orchestration_binding_id: Option<Uuid>,
    pub message: String,
}
