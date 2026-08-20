/// process-level facade for the REXRAP language-server protocol host.
#[derive(Default)]
pub struct LanguageServerService;

impl LanguageServerService {
    pub fn new() -> Self {
        Self
    }

    pub async fn run(self) {
        super::run_process().await;
    }
}
