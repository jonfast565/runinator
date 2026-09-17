#[allow(unused_imports)]
use super::*;

#[derive(Debug, Serialize, ToSchema)]
pub struct IngressResponse {
    pub admission_id: Uuid,
    pub generation: i64,
    pub disposition: String,
    pub duplicate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_position: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_binding_id: Option<Uuid>,
    pub message: String,
}
