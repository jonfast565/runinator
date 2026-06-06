use std::fmt;

pub type SendableError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub struct RuntimeError {
    code: String,
    message: String,
}

unsafe impl Sync for RuntimeError {}
unsafe impl Send for RuntimeError {}

impl RuntimeError {
    pub fn new(code: String, message: String) -> Self {
        Self { code, message }
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
        Box::new(RuntimeError::new(
            self.key.to_string(),
            format!("{} - {}: {detail}", self.code, self.summary),
        ))
    }

    /// like `error` but without an appended detail string.
    pub fn bare(&self) -> SendableError {
        Box::new(RuntimeError::new(
            self.key.to_string(),
            format!("{} - {}", self.code, self.summary),
        ))
    }
}

/// exposes a provider's full error dictionary for documentation and lookup.
pub trait ProviderErrors {
    /// every error this provider can emit, ordered by code.
    fn error_dictionary() -> &'static [ErrorDescriptor];
}
