#[allow(unused_imports)]
use super::*;

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
