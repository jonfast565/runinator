#[allow(unused_imports)]
use super::*;

pub trait ProcessRunner: Send + Sync {
    fn run(&self, request: ProcessRequest<'_>) -> Result<ProcessResult, ProcessFailure>;
}
