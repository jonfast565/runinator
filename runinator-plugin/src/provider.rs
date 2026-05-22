use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    providers::ProviderMetadata,
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};

use crate::cancel::CancellationToken;

pub trait ProviderEventSink: Send + Sync {
    fn emit(&self, event: ProviderExecutionEvent);
}

pub trait Provider: Send + Sync {
    fn name(&self) -> String;

    fn metadata(&self) -> ProviderMetadata;

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
        token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError>;
}
