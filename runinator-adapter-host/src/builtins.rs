//! host-side execution contract for compiled-in inbound adapters.
use super::*;
use std::{future::Future, pin::Pin, sync::OnceLock};

type PollFuture = Pin<Box<dyn Future<Output = AdapterPollResponse> + Send>>;
pub(super) trait BuiltinAdapter: Send + Sync {
    fn metadata(&self) -> AdapterKindMetadata;
    fn handle(&self, request: AdapterRequest, body_limit: usize) -> AdapterResponse;
    fn poll(&self, request: AdapterPollRequest) -> PollFuture {
        Box::pin(async move { unsupported_poll(request) })
    }
}
struct GenericWebhook;
struct Github;
struct Jira;
impl BuiltinAdapter for GenericWebhook {
    fn metadata(&self) -> AdapterKindMetadata {
        generic_metadata()
    }
    fn handle(&self, request: AdapterRequest, body_limit: usize) -> AdapterResponse {
        handle_generic(request, body_limit)
    }
}
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
pub(super) fn registry() -> &'static BTreeMap<String, Box<dyn BuiltinAdapter>> {
    static REGISTRY: OnceLock<BTreeMap<String, Box<dyn BuiltinAdapter>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let adapters: Vec<Box<dyn BuiltinAdapter>> =
            vec![Box::new(GenericWebhook), Box::new(Github), Box::new(Jira)];
        adapters
            .into_iter()
            .map(|adapter| (adapter.metadata().kind, adapter))
            .collect()
    })
}
pub(super) fn unsupported_poll(request: AdapterPollRequest) -> AdapterPollResponse {
    AdapterPollResponse {
        events: Vec::new(),
        checkpoint: request.checkpoint,
        retry_after_seconds: None,
        error: Some("adapter kind does not support polling".into()),
    }
}

#[cfg(test)]
#[path = "builtins_tests.rs"]
mod tests;
