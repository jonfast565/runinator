/// process-level facade for the stdio MCP request host.
#[derive(Default)]
pub struct McpService;

impl McpService {
    pub fn new() -> Self {
        Self
    }

    pub fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        super::run_process()
    }
}
