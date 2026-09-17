#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub struct RuntimeError {
    pub(super) code: String,
    pub(super) message: String,
    // stable numbered code (e.g. "JIRA001"), set only when built from an `ErrorDescriptor`. kept
    // separate from `code` (the dotted runtime key) so log call sites can attach it as a structured
    // field without parsing the rendered message.
    pub(super) numbered_code: Option<String>,
}

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
