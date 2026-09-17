#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct ValidateInvokeRequest {
    pub(super) kind: String,
    pub(super) request: AdapterValidationRequest,
}
