use std::fmt;

pub type SendableError = Box<dyn std::error::Error + Send + Sync + 'static>;

unsafe impl Sync for RuntimeError {}
unsafe impl Send for RuntimeError {}

/// pulls a stable numbered error code (e.g. "BROKER005") out of an arbitrary error for use as a
/// structured log field. tries a direct downcast to [`RuntimeError`] first (built via
/// [`ErrorDescriptor::error`]/[`ErrorDescriptor::bare`]); otherwise falls back to scanning the
/// rendered message for a code-shaped token, which covers `thiserror` enums that bake their code
/// directly into the `#[error(...)]` string (e.g. `"BROKER005 - ..."`).
pub fn extract_error_code(err: &(dyn std::error::Error + 'static)) -> Option<String> {
    if let Some(runtime_err) = err.downcast_ref::<RuntimeError>() {
        return runtime_err.numbered_code().map(str::to_string);
    }
    scan_for_code_token(&err.to_string())
}

/// like [`extract_error_code`], but returns `"UNKNOWN"` instead of `None` for the common case of
/// attaching an `error_code` structured log field in one expression.
pub fn error_code_or_unknown(err: &(dyn std::error::Error + 'static)) -> String {
    extract_error_code(err).unwrap_or_else(|| "UNKNOWN".to_string())
}

fn scan_for_code_token(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|token| is_code_token(token))
        .map(|token| token.to_string())
}

fn is_code_token(token: &str) -> bool {
    let Some(split_at) = token.find(|c: char| c.is_ascii_digit()) else {
        return false;
    };
    let (letters, digits) = token.split_at(split_at);
    letters.len() >= 2
        && digits.len() >= 2
        && letters.chars().all(|c| c.is_ascii_uppercase())
        && digits.chars().all(|c| c.is_ascii_digit())
}

/// exposes a provider's full error dictionary for documentation and lookup.

/// exposes an engine crate's full error dictionary for documentation and lookup.
/// the engine counterpart to [`ProviderErrors`]; entries share the `RUNI` prefix
/// with per-crate number ranges.

/// Portable workspace contract errors shared by storage and execution hosts.
pub const WORKSPACE_COMMIT_UNSUPPORTED: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE001",
    "workspace.commit.unsupported",
    "Store does not support atomic workspace commits",
);
pub const WORKSPACE_CONFLICT: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE002",
    "workspace.conflict",
    "Workspace version or checkout is no longer current",
);
pub const WORKSPACE_INVALID: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE003",
    "workspace.invalid",
    "Workspace content or reference is invalid",
);
pub const WORKSPACE_IO: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE004",
    "workspace.io",
    "Workspace storage I/O failure",
);
pub const WORKSPACE_JSON: ErrorDescriptor =
    ErrorDescriptor::new("WORKSPACE005", "workspace.json", "Invalid workspace JSON");
pub const WORKSPACE_CORRUPT: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE006",
    "workspace.corrupt",
    "Corrupt workspace storage",
);
pub const WORKSPACE_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE007",
    "workspace.missing",
    "Workspace object not found",
);
pub const WORKSPACE_EXISTS: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE008",
    "workspace.exists",
    "Workspace path already exists",
);
pub const WORKSPACE_BUSY: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE009",
    "workspace.busy",
    "Workspace storage is busy",
);
pub const WORKSPACE_CACHE_FULL: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE010",
    "workspace.cache_full",
    "Workspace cache budget exhausted",
);
pub const WORKSPACE_POISONED: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE011",
    "workspace.poisoned",
    "Workspace storage synchronization failed",
);
pub const WORKSPACE_COMMIT_UNCERTAIN: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE012",
    "workspace.commit_uncertain",
    "Workspace publication outcome is uncertain",
);
pub const WORKSPACE_LIMIT: ErrorDescriptor = ErrorDescriptor::new(
    "WORKSPACE013",
    "workspace.limit",
    "Workspace limit exceeded",
);
pub const WORKSPACE_DICTIONARY: &[ErrorDescriptor] = &[
    WORKSPACE_COMMIT_UNSUPPORTED,
    WORKSPACE_CONFLICT,
    WORKSPACE_INVALID,
    WORKSPACE_IO,
    WORKSPACE_JSON,
    WORKSPACE_CORRUPT,
    WORKSPACE_MISSING,
    WORKSPACE_EXISTS,
    WORKSPACE_BUSY,
    WORKSPACE_CACHE_FULL,
    WORKSPACE_POISONED,
    WORKSPACE_COMMIT_UNCERTAIN,
    WORKSPACE_LIMIT,
];

mod runtime_error;
pub use runtime_error::RuntimeError;

mod error_descriptor;
pub use error_descriptor::ErrorDescriptor;

mod provider_errors;
pub use provider_errors::ProviderErrors;

mod engine_errors;
pub use engine_errors::EngineErrors;
