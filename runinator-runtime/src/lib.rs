pub mod config;
pub mod errors;
pub mod machine;
pub mod orchestration;

// an in-memory `RuntimeStore` for driving node operations in a test. behind a feature so normal builds
// never compile it, mirroring `runinator-engine`'s `test-support`.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use machine::{DriveOutcome, DriveRequest, Suspension, WorkflowMachine};
pub use orchestration::{
    PipelineInquiryDecision, PipelineStartOutcome, ReadyNodeDisposition,
    create_and_start_pipeline_run, resolve_pipeline_run_inquiry, resume_pipeline_run,
    retry_pipeline_member, start_pipeline_run,
};

use runinator_models::{errors::SendableError, orchestration::ReadyNodeRecord};
use runinator_store::RuntimeStore;

/// Compatibility entry point for scheduler callers. New integrations may construct a
/// [`WorkflowMachine`] explicitly.
pub async fn process_ready_node<T: RuntimeStore>(
    store: &T,
    ready_node: &ReadyNodeRecord,
) -> Result<ReadyNodeDisposition, SendableError> {
    WorkflowMachine::new(store)
        .drive_ready(ready_node)
        .await
        .map(|outcome| match outcome {
            DriveOutcome::KeepClaim => ReadyNodeDisposition::KeepClaim,
            _ => ReadyNodeDisposition::Complete,
        })
}
