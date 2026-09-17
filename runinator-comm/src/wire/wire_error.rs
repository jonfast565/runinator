#[allow(unused_imports)]
use super::*;

/// error raised when a wire conversion fails.
#[derive(Debug)]
pub struct WireError(pub(super) serde_json::Error);

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} - wire codec error: {}",
            crate::errors::WIRE_CODEC.code,
            self.0
        )
    }
}

impl std::error::Error for WireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<serde_json::Error> for WireError {
    fn from(error: serde_json::Error) -> Self {
        Self(error)
    }
}
