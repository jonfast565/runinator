use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::SqlProvider;

// numbered error dictionary for the sql provider.
pub(crate) const UNSUPPORTED_CALL: ErrorDescriptor =
    ErrorDescriptor::new("SQL001", "UNSUPPORTED_CALL", "Unsupported provider call");
pub(crate) const INVALID_ARGUMENT: ErrorDescriptor =
    ErrorDescriptor::new("SQL002", "INVALID_ARGUMENT", "Invalid argument");
pub(crate) const QUERY_CANCELED: ErrorDescriptor =
    ErrorDescriptor::new("SQL003", "QUERY_CANCELED", "Query canceled");
pub(crate) const QUERY_TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("SQL004", "QUERY_TIMEOUT", "Query timed out");
pub(crate) const QUERY_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("SQL005", "QUERY_FAILED", "Query failed");

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    UNSUPPORTED_CALL,
    INVALID_ARGUMENT,
    QUERY_CANCELED,
    QUERY_TIMEOUT,
    QUERY_FAILED,
];

impl ProviderErrors for SqlProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
