use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::FunctionsProvider;

// numbered error dictionary for packaged-function execution.
pub(crate) const INVALID_REQUEST: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC001",
    "functions.invalid_request",
    "Invalid function invocation request",
);
pub(crate) const UNKNOWN_RUNTIME: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC002",
    "functions.unknown_runtime",
    "Unsupported function runtime",
);
pub(crate) const PACKAGE_UNREADABLE: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC003",
    "functions.package_unreadable",
    "Function package could not be read",
);
pub(crate) const INVOCATION_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC004",
    "functions.invocation_failed",
    "Function invocation failed",
);
pub(crate) const INVOCATION_TIMEOUT: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC005",
    "functions.invocation_timeout",
    "Function invocation timed out",
);
pub(crate) const INVOCATION_CANCELED: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC006",
    "functions.invocation_canceled",
    "Function invocation canceled",
);
pub(crate) const INVALID_OUTPUT: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC007",
    "functions.invalid_output",
    "Function returned invalid output",
);
pub(crate) const RUNTIME_UNAVAILABLE: ErrorDescriptor = ErrorDescriptor::new(
    "FUNC008",
    "functions.runtime_unavailable",
    "Container runtime unavailable on this worker",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_REQUEST,
    UNKNOWN_RUNTIME,
    PACKAGE_UNREADABLE,
    INVOCATION_FAILED,
    INVOCATION_TIMEOUT,
    INVOCATION_CANCELED,
    INVALID_OUTPUT,
    RUNTIME_UNAVAILABLE,
];

impl ProviderErrors for FunctionsProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
