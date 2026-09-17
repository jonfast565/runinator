#[allow(unused_imports)]
use super::*;

pub(super) struct Jira;

impl BuiltinAdapter for Jira {
    fn metadata(&self) -> AdapterKindMetadata {
        jira_metadata()
    }
    fn handle(&self, request: AdapterRequest, body_limit: usize) -> AdapterResponse {
        handle_jira(request, body_limit)
    }
    fn poll(&self, request: AdapterPollRequest) -> PollFuture {
        Box::pin(poll_jira(request))
    }
}
