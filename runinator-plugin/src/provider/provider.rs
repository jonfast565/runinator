#[allow(unused_imports)]
use super::*;

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
