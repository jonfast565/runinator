#[allow(unused_imports)]
use super::*;

pub trait InvocationRuntime: Send + Sync {
    fn name(&self) -> &'static str;

    fn invoke(
        &self,
        request: &InvocationRequest,
        logs: Option<Arc<dyn LineSink>>,
        token: CancellationToken,
    ) -> Result<InvocationOutcome, SendableError>;
}
