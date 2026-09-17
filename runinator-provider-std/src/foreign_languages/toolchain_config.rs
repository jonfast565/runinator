#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ToolchainConfig {
    pub(crate) executable: String,
    pub(crate) build_args: Vec<String>,
    pub(crate) run_args: Vec<String>,
}
