#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct WorkspaceAllocationRequest {
    pub admission_id: Uuid,
    pub generation: i64,
    pub scope: String,
    pub attempt: i64,
    pub required_labels: BTreeMap<String, String>,
    pub lease_seconds: Option<i64>,
}
