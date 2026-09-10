//! injected git command failures preserve the provider error vocabulary.
use super::*;
struct Canceled;
impl ProcessRunner for Canceled {
    fn run(
        &self,
        request: ProcessRequest<'_>,
    ) -> Result<runinator_provider_support::process_runner::ProcessResult, ProcessFailure> {
        assert_eq!(request.command.get_program(), "git");
        Err(ProcessFailure::Canceled)
    }
}
#[test]
fn maps_cancellation() {
    let error = run_command(
        &Canceled,
        "git",
        &["status"],
        10,
        &CancellationToken::new(),
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains(CANCELED.code));
}
