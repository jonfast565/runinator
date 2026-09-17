#[allow(unused_imports)]
use super::*;

pub(super) struct Runner;

impl ProcessRunner for Runner {
    fn run(&self, request: ProcessRequest<'_>) -> Result<ProcessResult, ProcessFailure> {
        assert_eq!(request.command.get_program(), "test-claude");
        let args: Vec<_> = request.command.get_args().collect();
        assert!(args.contains(&std::ffi::OsStr::new("prompt text")));
        assert_eq!(request.timeout, Duration::from_secs(5));
        Err(ProcessFailure::TimedOut)
    }
}
