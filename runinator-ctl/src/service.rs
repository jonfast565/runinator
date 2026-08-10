use crate::commands;

/// process-level facade for control CLI command dispatch.
#[derive(Default)]
pub struct CtlService;

impl CtlService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> commands::Result<()> {
        super::run_process().await
    }
}
