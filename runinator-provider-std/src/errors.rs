use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::StdProvider;

// numbered error dictionary for the std standard-library provider.
pub(crate) const INVALID_PROGRAM: ErrorDescriptor =
    ErrorDescriptor::new("STD001", "std.invalid_program", "Invalid compute program");
pub(crate) const INTRINSIC_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("STD002", "std.intrinsic_failed", "Intrinsic failed");
pub(crate) const HTTP_ERROR: ErrorDescriptor =
    ErrorDescriptor::new("STD003", "std.http_error", "HTTP request failed");
pub(crate) const GOTO_NOT_ALLOWED: ErrorDescriptor = ErrorDescriptor::new(
    "STD004",
    "std.goto_not_allowed",
    "goto is not allowed in an effectful exec program",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PROGRAM,
    INTRINSIC_FAILED,
    HTTP_ERROR,
    GOTO_NOT_ALLOWED,
];

impl ProviderErrors for StdProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
