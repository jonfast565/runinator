#[allow(unused_imports)]
use super::*;

pub struct WorkflowIngressContext<T> {
    pub db: Arc<T>,
    pub operations: Arc<RunOperations<T>>,
    pub caller_org_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub workflow_id: Uuid,
    pub request: PipelineIngressRequest,
    pub provenance: WorkflowRunProvenance,
    pub bypass_gate: bool,
}
