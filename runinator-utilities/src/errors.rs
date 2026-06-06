use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for cross-cutting utilities (RUNI6xx).

pub const FFI_NULL_STRING: ErrorDescriptor =
    ErrorDescriptor::new("RUNI601", "ffi.null_string", "FFI string pointer was null");
pub const CWD_EXECUTABLE_PARENT_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI602",
    "utilities.cwd.executable_parent_missing",
    "Executable path has no parent directory",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[FFI_NULL_STRING, CWD_EXECUTABLE_PARENT_MISSING];

/// utilities error dictionary.
pub struct UtilitiesErrors;

impl EngineErrors for UtilitiesErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
