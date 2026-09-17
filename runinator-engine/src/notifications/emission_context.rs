#[allow(unused_imports)]
use super::*;

pub(super) struct EmissionContext {
    pub(super) workflow_run_id: Option<Uuid>,
    pub(super) node_id: Option<String>,
    pub(super) title: String,
    pub(super) body: String,
    pub(super) metadata: Value,
    /// distinguishes one logical occurrence, so re-evaluating the same condition is idempotent.
    pub(super) occurrence: String,
}
