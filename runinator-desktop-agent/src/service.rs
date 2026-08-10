/// process-level facade for the exclusive desktop worker application.
#[derive(Default)]
pub struct DesktopAgentService;

impl DesktopAgentService {
    pub fn new() -> Self {
        Self
    }

    pub fn run(self) -> eframe::Result<()> {
        super::run_process()
    }
}
