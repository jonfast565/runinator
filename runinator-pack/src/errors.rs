use runinator_models::errors::{EngineErrors, ErrorDescriptor};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, PackError>;

/// errors produced while discovering and compiling a pack source into a bundle.
#[derive(Debug, Error)]
pub enum PackError {
    /// a filesystem read failed.
    #[error("PACK001 - io error: {0}")]
    Io(#[from] std::io::Error),

    /// a manifest or settings file was not valid json.
    #[error("PACK002 - json error: {0}")]
    Json(#[from] serde_json::Error),

    /// formatting, compiling, or parsing a wdl/wdls source failed (carries the rendered diagnostic).
    #[error("PACK003 - compile error: {0}")]
    Compile(String),

    /// the pack source was structurally invalid (unsupported extension, empty directory, or a
    /// malformed `.wdlm` manifest).
    #[error("PACK004 - pack source error: {0}")]
    Source(String),
}

impl PackError {
    pub(crate) fn compile(message: impl Into<String>) -> Self {
        Self::Compile(message.into())
    }

    pub(crate) fn source(message: impl Into<String>) -> Self {
        Self::Source(message.into())
    }
}

// numbered error dictionary for pack source compilation.
pub const IO: ErrorDescriptor = ErrorDescriptor::new("PACK001", "pack.io", "IO error");
pub const JSON: ErrorDescriptor = ErrorDescriptor::new("PACK002", "pack.json", "JSON error");
pub const COMPILE: ErrorDescriptor =
    ErrorDescriptor::new("PACK003", "pack.compile", "Compile error");
pub const SOURCE: ErrorDescriptor =
    ErrorDescriptor::new("PACK004", "pack.source", "Pack source error");

pub const DICTIONARY: &[ErrorDescriptor] = &[IO, JSON, COMPILE, SOURCE];

impl EngineErrors for PackError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
