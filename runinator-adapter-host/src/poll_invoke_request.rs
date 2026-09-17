#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct PollInvokeRequest {
    pub(super) kind: String,
    pub(super) request: AdapterPollRequest,
}
