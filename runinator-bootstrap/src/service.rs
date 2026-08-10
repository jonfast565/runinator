use std::process::ExitCode;

/// process-level facade for the one-shot database bootstrap operation.
#[derive(Default)]
pub struct BootstrapService;

impl BootstrapService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> ExitCode {
        super::run_process().await
    }
}
