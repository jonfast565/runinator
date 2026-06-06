use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the communication contracts crate.
pub const WIRE_CODEC: ErrorDescriptor = ErrorDescriptor::new(
    "COMM001",
    "comm.wire_codec",
    "Wire codec serialization failed",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[WIRE_CODEC];

/// communication contracts error dictionary.
pub struct CommErrors;

impl EngineErrors for CommErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
