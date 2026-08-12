use crate::config::AgentConfig;

/// process-level facade for the windowed desktop agent. the headless shape has its own entry point
/// (`crate::headless`) because it never builds a window at all.
#[derive(Default)]
pub struct DesktopAgentService;

impl DesktopAgentService {
    pub fn new() -> Self {
        Self
    }

    pub fn run(self, config: AgentConfig) -> eframe::Result<()> {
        super::run_gui(config)
    }
}
