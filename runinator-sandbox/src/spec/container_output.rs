#[allow(unused_imports)]
use super::*;

/// what a completed container produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    /// set when output was dropped, so a caller can say so rather than presenting a partial log as
    /// the whole of one.
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub duration: Duration,
}

impl ContainerOutput {
    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}
