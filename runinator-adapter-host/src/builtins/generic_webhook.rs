#[allow(unused_imports)]
use super::*;

pub(super) struct GenericWebhook;

impl BuiltinAdapter for GenericWebhook {
    fn metadata(&self) -> AdapterKindMetadata {
        generic_metadata()
    }
    fn handle(&self, request: AdapterRequest, body_limit: usize) -> AdapterResponse {
        handle_generic(request, body_limit)
    }
}
