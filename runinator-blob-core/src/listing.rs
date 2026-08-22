//! the ListObjectsV2 request and response shapes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// the default page size when a caller sends no `max-keys`, matching S3.
pub const DEFAULT_MAX_KEYS: usize = 1000;

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

/// one bucket in a `ListBuckets` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketSummary {
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// one object in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectSummary {
    pub key: String,
    pub size: u64,
    pub sha256: String,
    pub last_modified: DateTime<Utc>,
}

/// one page of a listing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListResponse {
    pub objects: Vec<ObjectSummary>,
    /// prefixes rolled up by the delimiter, if one was supplied.
    #[serde(default)]
    pub common_prefixes: Vec<String>,
    pub is_truncated: bool,
    /// the token that fetches the next page; `None` when this page is the last.
    #[serde(default)]
    pub next_continuation_token: Option<String>,
}
