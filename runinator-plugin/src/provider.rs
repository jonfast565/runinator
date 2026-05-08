use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};

pub trait ProviderEventSink: Send + Sync {
    fn emit(&self, event: ProviderExecutionEvent);
}

pub trait Provider: Send + Sync {
    fn name(&self) -> String;

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError>;
}
