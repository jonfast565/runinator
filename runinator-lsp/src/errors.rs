#![allow(dead_code)]
//! numbered error dictionary for the language server. bins flag unused `pub` items, hence the
//! module-level allow.

use runinator_models::errors::{EngineErrors, ErrorDescriptor};

pub const POSITION: ErrorDescriptor =
    ErrorDescriptor::new("LSP001", "lsp.position", "Position mapping error");
pub const METADATA: ErrorDescriptor =
    ErrorDescriptor::new("LSP002", "lsp.metadata", "Metadata fetch failed");
pub const APPLY_COMPILE: ErrorDescriptor =
    ErrorDescriptor::new("LSP003", "lsp.apply.compile", "Auto-apply compile failed");
pub const APPLY_IMPORT: ErrorDescriptor =
    ErrorDescriptor::new("LSP004", "lsp.apply.import", "Auto-apply import failed");
pub const CONFIG: ErrorDescriptor =
    ErrorDescriptor::new("LSP005", "lsp.config", "Config parse error");

pub const DICTIONARY: &[ErrorDescriptor] =
    &[POSITION, METADATA, APPLY_COMPILE, APPLY_IMPORT, CONFIG];

/// engine-error dictionary handle for the language server.
pub struct LspErrors;

impl EngineErrors for LspErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
