#[allow(unused_imports)]
use super::*;

/// Output retained while both streams were also emitted to the provider event sink.
#[derive(Debug, Default)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
}
