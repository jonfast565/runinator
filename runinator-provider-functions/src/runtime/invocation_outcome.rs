#[allow(unused_imports)]
use super::*;

/// what one invocation produced.
#[derive(Debug, Clone)]
pub struct InvocationOutcome {
    pub output: Value,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub duration: Duration,
}
