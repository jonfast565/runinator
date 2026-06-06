use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::ConsoleProvider;

// numbered error dictionary for the console provider.
pub(crate) const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("CONSOLE001", "console.invalid_params", "Invalid parameters");
pub(crate) const IO: ErrorDescriptor =
    ErrorDescriptor::new("CONSOLE002", "console.io", "I/O error");
pub(crate) const STDOUT_UNAVAILABLE: ErrorDescriptor = ErrorDescriptor::new(
    "CONSOLE003",
    "console.stdout.unavailable",
    "Failed to capture command stdout",
);
pub(crate) const STDERR_UNAVAILABLE: ErrorDescriptor = ErrorDescriptor::new(
    "CONSOLE004",
    "console.stderr.unavailable",
    "Failed to capture command stderr",
);
pub(crate) const NONZERO_EXIT: ErrorDescriptor = ErrorDescriptor::new(
    "CONSOLE005",
    "console.nonzero_exit",
    "Command exited with a non-zero status",
);
pub(crate) const CANCELED: ErrorDescriptor =
    ErrorDescriptor::new("CONSOLE006", "console.canceled", "Command canceled");
pub(crate) const TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("CONSOLE007", "console.timeout", "Command timed out");

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    IO,
    STDOUT_UNAVAILABLE,
    STDERR_UNAVAILABLE,
    NONZERO_EXIT,
    CANCELED,
    TIMEOUT,
];

impl ProviderErrors for ConsoleProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
