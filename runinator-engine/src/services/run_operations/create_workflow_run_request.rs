#[allow(unused_imports)]
use super::*;

pub struct CreateWorkflowRunRequest {
    pub workflow_id: Uuid,
    pub parameters: Value,
    pub debug: bool,
    pub name: Option<String>,
    pub provenance: WorkflowRunProvenance,
    pub file_ids: Vec<Uuid>,
    pub org_id: Option<Uuid>,
    pub principal_id: Option<Uuid>,
}
