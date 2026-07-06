use std::fmt;

pub type SendableError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub struct RuntimeError {
    code: String,
    message: String,
    // stable numbered code (e.g. "JIRA001"), set only when built from an `ErrorDescriptor`. kept
    // separate from `code` (the dotted runtime key) so log call sites can attach it as a structured
    // field without parsing the rendered message.
    numbered_code: Option<String>,
}

unsafe impl Sync for RuntimeError {}
unsafe impl Send for RuntimeError {}

impl RuntimeError {
    pub fn new(code: String, message: String) -> Self {
        Self {
            code,
            message,
            numbered_code: None,
        }
    }

    /// the stable numbered code (e.g. "JIRA001") this error was raised from, if it was built via
    /// [`ErrorDescriptor::error`] or [`ErrorDescriptor::bare`].
    pub fn numbered_code(&self) -> Option<&str> {
        self.numbered_code.as_deref()
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

/// one entry in a provider's error dictionary: a stable numbered code, the
/// dotted runtime code it maps to, and a short human summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorDescriptor {
    /// stable numbered code, e.g. "JIRA001".
    pub code: &'static str,
    /// dotted runtime key kept for back-compat lookups, e.g. "jira.config".
    pub key: &'static str,
    /// short human summary, e.g. "Could not parse URL".
    pub summary: &'static str,
}

impl ErrorDescriptor {
    pub const fn new(code: &'static str, key: &'static str, summary: &'static str) -> Self {
        Self { code, key, summary }
    }

    /// builds an error like "JIRA001 - Could not parse URL: <detail>" while
    /// keeping the dotted key as the runtime error code.
    pub fn error(&self, detail: impl fmt::Display) -> SendableError {
        Box::new(RuntimeError {
            code: self.key.to_string(),
            message: format!("{} - {}: {detail}", self.code, self.summary),
            numbered_code: Some(self.code.to_string()),
        })
    }

    /// like `error` but without an appended detail string.
    pub fn bare(&self) -> SendableError {
        Box::new(RuntimeError {
            code: self.key.to_string(),
            message: format!("{} - {}", self.code, self.summary),
            numbered_code: Some(self.code.to_string()),
        })
    }
}

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
pub trait ProviderErrors {
    /// every error this provider can emit, ordered by code.
    fn error_dictionary() -> &'static [ErrorDescriptor];
}

/// exposes an engine crate's full error dictionary for documentation and lookup.
/// the engine counterpart to [`ProviderErrors`]; entries share the `RUNI` prefix
/// with per-crate number ranges.
pub trait EngineErrors {
    /// every error this engine crate can emit, ordered by code.
    fn error_dictionary() -> &'static [ErrorDescriptor];
}
