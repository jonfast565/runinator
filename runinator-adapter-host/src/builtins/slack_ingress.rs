#[allow(unused_imports)]
use super::*;

pub(super) struct SlackIngress;

impl BuiltinAdapter for SlackIngress {
    fn metadata(&self) -> AdapterKindMetadata {
        slack_ingress_metadata()
    }
    fn handle(&self, request: AdapterRequest, body_limit: usize) -> AdapterResponse {
        handle_slack_ingress(request, body_limit)
    }
}
