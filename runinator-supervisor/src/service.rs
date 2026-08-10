use crate::types::DynError;

/// process-level facade for supervisor command dispatch and daemon lifecycle.
#[derive(Default)]
pub struct SupervisorService;

impl SupervisorService {
    pub fn new() -> Self {
        Self
    }

    pub fn run(self) -> Result<(), DynError> {
        super::run_process()
    }
}
