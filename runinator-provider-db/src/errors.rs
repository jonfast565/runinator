use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::DbProvider;

// numbered error dictionary for the database provider.
pub(crate) const UNSUPPORTED_CALL: ErrorDescriptor =
    ErrorDescriptor::new("DB001", "UNSUPPORTED_CALL", "Unsupported provider call");
pub(crate) const INVALID_ARGUMENT: ErrorDescriptor =
    ErrorDescriptor::new("DB002", "INVALID_ARGUMENT", "Invalid argument");
pub(crate) const INVALID_STATEMENT: ErrorDescriptor = ErrorDescriptor::new(
    "DB003",
    "INVALID_STATEMENT",
    "Statement is not valid for this engine",
);
pub(crate) const CONNECTION_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("DB004", "CONNECTION_FAILED", "Database connection failed");
pub(crate) const DATABASE_MISSING: ErrorDescriptor =
    ErrorDescriptor::new("DB005", "DATABASE_MISSING", "Database does not exist");
pub(crate) const STATEMENT_TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("DB006", "STATEMENT_TIMEOUT", "Statement timed out");
pub(crate) const STATEMENT_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("DB007", "STATEMENT_FAILED", "Statement failed");
pub(crate) const TRANSACTION_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("DB008", "TRANSACTION_FAILED", "Transaction failed");
pub(crate) const STATEMENT_CANCELED: ErrorDescriptor =
    ErrorDescriptor::new("DB009", "STATEMENT_CANCELED", "Statement canceled");
pub(crate) const EXPORT_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("DB010", "EXPORT_FAILED", "Result export failed");
pub(crate) const UNSUPPORTED_ENGINE: ErrorDescriptor = ErrorDescriptor::new(
    "DB011",
    "UNSUPPORTED_ENGINE",
    "Operation is unsupported for this engine",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    UNSUPPORTED_CALL,
    INVALID_ARGUMENT,
    INVALID_STATEMENT,
    CONNECTION_FAILED,
    DATABASE_MISSING,
    STATEMENT_TIMEOUT,
    STATEMENT_FAILED,
    TRANSACTION_FAILED,
    STATEMENT_CANCELED,
    EXPORT_FAILED,
    UNSUPPORTED_ENGINE,
];

impl ProviderErrors for DbProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
