// the dictionary doubles as documentation; some entries are only reachable when the archiver is
// configured to register with the web service, so allow unused items in this bin crate.
#![allow(dead_code)]

use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the archiver engine (RUNI7xx).

pub const REPLICA_REGISTER: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI701",
    "archiver.replica.register",
    "Failed to register archiver replica",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[REPLICA_REGISTER];

/// archiver engine error dictionary.
pub struct ArchiverErrors;

impl EngineErrors for ArchiverErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
