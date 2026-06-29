use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::LocalProvider;

// numbered error dictionary for the local-files provider.
pub(crate) const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("LOCALFS001", "localfs.invalid_params", "Invalid parameters");
pub(crate) const ROOT_NOT_CONFIGURED: ErrorDescriptor = ErrorDescriptor::new(
    "LOCALFS002",
    "localfs.root.not_configured",
    "Local files root is not configured",
);
pub(crate) const PATH_OUTSIDE_ROOT: ErrorDescriptor = ErrorDescriptor::new(
    "LOCALFS003",
    "localfs.path.outside_root",
    "Path escapes the configured local files root",
);
pub(crate) const NOT_FOUND: ErrorDescriptor =
    ErrorDescriptor::new("LOCALFS004", "localfs.not_found", "Path does not exist");
pub(crate) const IO: ErrorDescriptor =
    ErrorDescriptor::new("LOCALFS005", "localfs.io", "I/O error");
pub(crate) const WRITE_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "LOCALFS006",
    "localfs.write.disabled",
    "Writes are disabled for this local files worker",
);
pub(crate) const UNKNOWN_ACTION: ErrorDescriptor = ErrorDescriptor::new(
    "LOCALFS007",
    "localfs.unknown_action",
    "Unknown local files action",
);
pub(crate) const NOT_A_FILE: ErrorDescriptor = ErrorDescriptor::new(
    "LOCALFS008",
    "localfs.not_a_file",
    "Path is not a regular file",
);
pub(crate) const NOT_A_DIRECTORY: ErrorDescriptor = ErrorDescriptor::new(
    "LOCALFS009",
    "localfs.not_a_directory",
    "Path is not a directory",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    ROOT_NOT_CONFIGURED,
    PATH_OUTSIDE_ROOT,
    NOT_FOUND,
    IO,
    WRITE_DISABLED,
    UNKNOWN_ACTION,
    NOT_A_FILE,
    NOT_A_DIRECTORY,
];

impl ProviderErrors for LocalProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
