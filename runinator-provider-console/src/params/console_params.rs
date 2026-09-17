#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub(crate) struct ConsoleParams {
    pub command: String,
    // run in a worker-owned PTY/ConPTY so Command Center can render and drive the live terminal.
    // defaults to false: capture stdout and stderr independently.
    #[serde(default)]
    pub interactive: bool,
}
