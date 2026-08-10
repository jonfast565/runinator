pub mod config;
pub mod errors;
pub mod orchestration;

// an in-memory `ReducerStore` for driving handlers in a test. behind a feature so normal builds
// never compile it, mirroring `runinator-engine`'s `test-support`.
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use orchestration::{
    PipelineInquiryDecision, ReadyNodeDisposition, create_and_start_pipeline_run,
    process_ready_node, resolve_pipeline_run_inquiry, start_pipeline_run,
};
