#[allow(unused_imports)]
use super::*;

pub(crate) struct WorkflowIngressContext<T> {
    pub(crate) db: Arc<T>,
    pub(crate) operations: Arc<RunOperations<T>>,
    pub(crate) caller_org_id: Option<Uuid>,
    pub(crate) actor_id: Option<Uuid>,
    pub(crate) workflow_id: Uuid,
    pub(crate) request: IngressEventRequest,
    pub(crate) provenance: WorkflowRunProvenance,
    pub(crate) bypass_gate: bool,
}
