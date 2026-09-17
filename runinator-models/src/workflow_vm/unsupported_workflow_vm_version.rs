#[allow(unused_imports)]
use super::*;

/// A persisted or wire record was produced by a VM revision this process does not understand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedWorkflowVmVersion {
    pub record: WorkflowVmRecordKind,
    pub expected: u32,
    pub actual: u32,
}

impl std::fmt::Display for UnsupportedWorkflowVmVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "unsupported workflow VM {} version {}; expected {}",
            self.record, self.actual, self.expected
        )
    }
}

impl std::error::Error for UnsupportedWorkflowVmVersion {}
