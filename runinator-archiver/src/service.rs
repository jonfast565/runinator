use std::process::ExitCode;

/// process-level facade for durable history archival.
#[derive(Default)]
pub struct ArchiverService;

impl ArchiverService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> ExitCode {
        super::run_process().await
    }
}
