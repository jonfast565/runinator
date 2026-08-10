use runinator_models::errors::SendableError;

/// process-level facade for the Runinator HTTP and WebSocket service.
#[derive(Default)]
pub struct WebService;

impl WebService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<(), SendableError> {
        super::run_process().await
    }
}
