#[allow(unused_imports)]
use super::*;

/// Error classification retained across the engine/HTTP boundary for malformed setting entries.
#[derive(Debug)]
pub struct PackImportError {
    pub bad_request: bool,
    pub message: String,
}

impl PackImportError {
    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self {
            bad_request: false,
            message: message.into(),
        }
    }
}
