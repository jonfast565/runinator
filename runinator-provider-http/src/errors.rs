use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::HttpProvider;

pub(crate) const INVALID_PARAMS: ErrorDescriptor = ErrorDescriptor::new(
    "HTTP001",
    "http.invalid_params",
    "Invalid HTTP request parameters",
);
pub(crate) const FORBIDDEN_TARGET: ErrorDescriptor = ErrorDescriptor::new(
    "HTTP002",
    "http.forbidden_target",
    "HTTP target is not permitted",
);
pub(crate) const REQUEST_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("HTTP003", "http.request_failed", "HTTP request failed");
pub(crate) const UNEXPECTED_STATUS: ErrorDescriptor = ErrorDescriptor::new(
    "HTTP004",
    "http.unexpected_status",
    "HTTP response status was not expected",
);
pub(crate) const INVALID_RESPONSE: ErrorDescriptor = ErrorDescriptor::new(
    "HTTP005",
    "http.invalid_response",
    "HTTP response was invalid",
);

const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    FORBIDDEN_TARGET,
    REQUEST_FAILED,
    UNEXPECTED_STATUS,
    INVALID_RESPONSE,
];

impl ProviderErrors for HttpProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
