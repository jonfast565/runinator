#[allow(unused_imports)]
use super::*;

pub(crate) struct Options {
    /// expose every saved workflow as a tool of its own.
    pub workflow_tools: bool,
    /// Do not expose raw command execution or non-mission controls to a harnessed agent session.
    pub mission_only: bool,
    /// Optional binding fence for an MCP server running inside one mission phase.
    pub mission_id: Option<Uuid>,
    /// the default ceiling on one command.
    pub timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            workflow_tools: false,
            mission_only: false,
            mission_id: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}
