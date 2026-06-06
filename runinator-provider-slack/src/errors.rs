use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::SlackProvider;

// numbered error dictionary for the slack provider.
pub(crate) const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("SLACK001", "slack.invalid_params", "Invalid parameters");
pub(crate) const UNSUPPORTED_ACTION: ErrorDescriptor =
    ErrorDescriptor::new("SLACK002", "slack.unsupported_action", "Unsupported action");
pub(crate) const HTTP_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "SLACK003",
    "slack.http_error",
    "Slack returned an error status",
);
pub(crate) const INVALID_JSON: ErrorDescriptor = ErrorDescriptor::new(
    "SLACK004",
    "slack.invalid_json",
    "Response was not valid JSON",
);
pub(crate) const API_ERROR: ErrorDescriptor =
    ErrorDescriptor::new("SLACK005", "slack.api_error", "Slack API returned an error");

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    UNSUPPORTED_ACTION,
    HTTP_ERROR,
    INVALID_JSON,
    API_ERROR,
];

impl ProviderErrors for SlackProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
