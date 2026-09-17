#[allow(unused_imports)]
use super::*;

pub(super) struct Outcome {
    pub(super) status: WorkflowStatus,
    pub(super) output: Option<Value>,
    pub(super) note: Option<String>,
    pub(super) route_override: Option<String>,
    pub(super) force_terminal: bool,
}

impl Outcome {
    pub(super) fn new(status: WorkflowStatus, output: Option<Value>, note: &str) -> Self {
        Self {
            status,
            output,
            note: Some(note.to_string()),
            route_override: None,
            force_terminal: false,
        }
    }

    pub(super) fn plain(status: WorkflowStatus) -> Self {
        Self {
            status,
            output: None,
            note: None,
            route_override: None,
            force_terminal: false,
        }
    }
}
