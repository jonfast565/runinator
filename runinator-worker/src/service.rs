use runinator_models::errors::SendableError;

/// process-level facade for configuring and running the standalone worker.
#[derive(Default)]
pub struct WorkerService;

impl WorkerService {
    pub fn new() -> Self {
        Self
    }

    pub fn run(self) -> Result<(), SendableError> {
        super::run_process()
    }
}
