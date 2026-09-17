#[allow(unused_imports)]
use super::*;

/// one `runinatorctl` command, as a tool.
#[derive(Debug, Clone)]
pub(crate) struct CommandTool {
    /// the words that select the command, e.g. `["workflows", "apply"]`.
    pub path: Vec<String>,
    pub name: String,
    pub description: String,
    pub arguments: Vec<ToolArgument>,
}
