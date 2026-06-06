use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::GitHubProvider;

// numbered error dictionary for the github provider.
pub(crate) const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("GITHUB001", "github.invalid_params", "Invalid parameters");
pub(crate) const INVALID_JSON: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB002",
    "github.invalid_json",
    "Response was not valid JSON",
);
pub(crate) const HTTP_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB003",
    "github.http_error",
    "GitHub returned an error status",
);
pub(crate) const UNSUPPORTED_ACTION: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB004",
    "github.unsupported_action",
    "Unsupported action",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] =
    &[INVALID_PARAMS, INVALID_JSON, HTTP_ERROR, UNSUPPORTED_ACTION];

impl ProviderErrors for GitHubProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
