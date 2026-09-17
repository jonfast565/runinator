#[allow(unused_imports)]
use super::*;

/// a resolved `$ref`: a source root plus a path into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowValueRef {
    pub source: WorkflowRefSource,
    pub path: Vec<WorkflowPathSegment>,
}
