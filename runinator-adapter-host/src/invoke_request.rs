#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct InvokeRequest {
    pub(super) kind: String,
    pub(super) request: AdapterRequest,
}
