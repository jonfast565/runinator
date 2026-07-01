use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the plugin loader engine (RUNI4xx).

pub const ABI_UNSUPPORTED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI401",
    "plugin.abi.unsupported",
    "Unsupported plugin ABI version",
);
pub const EXECUTION_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI402",
    "plugin.v2.execution_failed",
    "Plugin execution failed",
);
pub const METADATA_INVALID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI403",
    "plugin.metadata.invalid",
    "Plugin metadata is invalid",
);
pub const PATH_INVALID: ErrorDescriptor =
    ErrorDescriptor::new("RUNI404", "plugin.path.invalid", "Plugin path is invalid");
pub const MARKER_INVALID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI405",
    "plugin.marker.invalid",
    "Plugin marker function returned an unexpected value",
);
pub const LOAD_FAILED: ErrorDescriptor =
    ErrorDescriptor::new("RUNI406", "plugin.load.failed", "Plugin load failed");

pub const DICTIONARY: &[ErrorDescriptor] = &[
    ABI_UNSUPPORTED,
    EXECUTION_FAILED,
    METADATA_INVALID,
    PATH_INVALID,
    MARKER_INVALID,
    LOAD_FAILED,
];

/// plugin loader engine error dictionary.
pub struct PluginErrors;

impl EngineErrors for PluginErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
