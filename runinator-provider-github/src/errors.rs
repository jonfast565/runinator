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

pub(crate) const MISSING_REVIEWERS: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB005",
    "github.missing_reviewers",
    "request_reviewers needs at least one reviewer or team_reviewer",
);
pub(crate) const MISSING_OPERATION_KEY: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB006",
    "github.missing_operation_key",
    "Reconcilable action needs an operation key",
);
pub(crate) const REVISION_MISMATCH: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUB007",
    "github.revision_mismatch",
    "GitHub returned a check run for a different revision",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    INVALID_JSON,
    HTTP_ERROR,
    UNSUPPORTED_ACTION,
    MISSING_REVIEWERS,
    MISSING_OPERATION_KEY,
    REVISION_MISMATCH,
];

impl ProviderErrors for GitHubProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
