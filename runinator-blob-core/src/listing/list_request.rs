#[allow(unused_imports)]
use super::*;

/// a listing query.
#[derive(Debug, Clone, Default)]
pub struct ListRequest {
    pub prefix: Option<String>,
    /// when set, keys sharing a prefix up to the next delimiter collapse into a common prefix
    /// instead of being listed individually.
    pub delimiter: Option<String>,
    /// resume token from a previous truncated page.
    pub continuation_token: Option<String>,
    pub max_keys: Option<usize>,
}

impl ListRequest {
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: Some(prefix.into()),
            ..Self::default()
        }
    }

    /// the effective page size, clamped to the S3 maximum.
    pub fn effective_max_keys(&self) -> usize {
        self.max_keys
            .unwrap_or(DEFAULT_MAX_KEYS)
            .clamp(1, DEFAULT_MAX_KEYS)
    }
}
