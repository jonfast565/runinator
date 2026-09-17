#[allow(unused_imports)]
use super::*;

pub(super) struct Github;

impl BuiltinAdapter for Github {
    fn metadata(&self) -> AdapterKindMetadata {
        github_metadata()
    }
    fn handle(&self, request: AdapterRequest, body_limit: usize) -> AdapterResponse {
        handle_github(request, body_limit)
    }
    fn poll(&self, request: AdapterPollRequest) -> PollFuture {
        Box::pin(poll_github(request))
    }
}
