#[allow(unused_imports)]
use super::*;

/// what a caller may ask for while writing an object.
#[derive(Debug, Clone, Default)]
pub struct PutOptions {
    pub content_type: Option<String>,
    pub metadata: BTreeMap<String, String>,
    /// reject the write when the key already exists (`If-None-Match: *`). this is what makes a
    /// content-addressed store write-once without a read-then-write race.
    pub if_none_match: bool,
    /// verify the body against this lowercase hex sha-256 before committing it.
    pub expected_sha256: Option<String>,
}

impl PutOptions {
    /// the options a content-addressed write wants: write-once, verified against its own digest.
    pub fn content_addressed(sha256: impl Into<String>) -> Self {
        Self {
            if_none_match: true,
            expected_sha256: Some(sha256.into()),
            ..Self::default()
        }
    }
}
