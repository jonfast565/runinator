/// process-level facade for the standalone broker server.
#[derive(Default)]
pub struct BrokerService;

impl BrokerService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        super::run_process().await
    }
}
