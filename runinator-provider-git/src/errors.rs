use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::GitProvider;

// numbered error dictionary for the git provider.
pub(crate) const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("GIT001", "git.invalid_params", "Invalid parameters");
pub(crate) const UNSUPPORTED_ACTION: ErrorDescriptor =
    ErrorDescriptor::new("GIT002", "git.unsupported_action", "Unsupported action");
pub(crate) const CANCELED: ErrorDescriptor =
    ErrorDescriptor::new("GIT003", "command.canceled", "Command canceled");
pub(crate) const TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("GIT004", "command.timeout", "Command timed out");
pub(crate) const NONZERO_EXIT: ErrorDescriptor = ErrorDescriptor::new(
    "GIT005",
    "command.nonzero_exit",
    "Command exited with a non-zero status",
);
pub(crate) const WORKSPACE_SAFETY: ErrorDescriptor = ErrorDescriptor::new(
    "GIT006",
    "git.workspace_safety",
    "Workspace failed a safety check",
);
pub(crate) const REVISION_MISMATCH: ErrorDescriptor = ErrorDescriptor::new(
    "GIT007",
    "git.revision_mismatch",
    "Git revision guard did not match",
);
pub(crate) const IO_ERROR: ErrorDescriptor =
    ErrorDescriptor::new("GIT008", "git.io", "Git artifact I/O failed");

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    UNSUPPORTED_ACTION,
    CANCELED,
    TIMEOUT,
    NONZERO_EXIT,
    WORKSPACE_SAFETY,
    REVISION_MISMATCH,
    IO_ERROR,
];

impl ProviderErrors for GitProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
