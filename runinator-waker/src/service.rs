use runinator_models::errors::SendableError;

/// process-level facade for the broker-only wake relay.
#[derive(Default)]
pub struct WakerService;

impl WakerService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<(), SendableError> {
        super::run_process().await
    }
}
