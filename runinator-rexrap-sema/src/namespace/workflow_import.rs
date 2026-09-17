#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct WorkflowImport {
    pub(super) path: String,
    pub(super) revision: Option<i64>,
}
