#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default)]
pub struct PipelineExecutionContext {
    pub requested_run_id: Option<Uuid>,
    pub orchestration_binding_id: Option<Uuid>,
    pub execution_epoch: Option<i64>,
    pub start_member: Option<String>,
}
