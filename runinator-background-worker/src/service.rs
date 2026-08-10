use runinator_models::errors::SendableError;

/// process-level facade for the standalone durable orchestration engine.
#[derive(Default)]
pub struct BackgroundWorkerService;

impl BackgroundWorkerService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<(), SendableError> {
        super::run_process().await
    }
}
