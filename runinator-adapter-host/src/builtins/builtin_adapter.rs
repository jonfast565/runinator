#[allow(unused_imports)]
use super::*;

pub(crate) trait BuiltinAdapter: Send + Sync {
    fn metadata(&self) -> AdapterKindMetadata;
    fn validate(&self, request: AdapterValidationRequest) -> AdapterValidationResponse {
        validate_configuration(&self.metadata(), request)
    }
    fn handle(&self, request: AdapterRequest, body_limit: usize) -> AdapterResponse;
    fn poll(&self, request: AdapterPollRequest) -> PollFuture {
        Box::pin(async move { unsupported_poll(request) })
    }
}
