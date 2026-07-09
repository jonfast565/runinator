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
pub(crate) const INTERACTIVE_NOT_PERMITTED: ErrorDescriptor = ErrorDescriptor::new(
    "CONSOLE008",
    "console.interactive_not_permitted",
    "Interactive console is only available on a desktop worker agent",
);
pub(crate) const WORKING_DIR_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "CONSOLE009",
    "console.working_dir.missing",
    "Configured console working directory does not exist",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    IO,
    STDOUT_UNAVAILABLE,
    STDERR_UNAVAILABLE,
    NONZERO_EXIT,
    CANCELED,
    TIMEOUT,
    INTERACTIVE_NOT_PERMITTED,
    WORKING_DIR_MISSING,
];

impl ProviderErrors for ConsoleProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
