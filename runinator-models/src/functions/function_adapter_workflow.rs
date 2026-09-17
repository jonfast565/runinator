#[allow(unused_imports)]
use super::*;

/// an adapter workflow generated for one export, so a direct http invocation runs through the same
/// reducer path a workflow call does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionAdapterWorkflow {
    pub id: Uuid,
    pub export_id: Uuid,
    pub workflow_id: Uuid,
    pub created_at: DateTime<Utc>,
}
