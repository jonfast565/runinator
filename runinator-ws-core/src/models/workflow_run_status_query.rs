#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusQuery {
    pub status: Option<WorkflowStatus>,
    pub workflow_id: Option<Uuid>,
    pub name: Option<String>,
    pub open: Option<bool>,
    /// caps the unfiltered recent-runs list; clamped server-side. absent uses the default cap.
    pub limit: Option<i64>,
}
