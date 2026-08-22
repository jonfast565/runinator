//! what the store knows about an object besides its bytes.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::range::ResolvedRange;

/// Default content type for an object that has none, matching S3.
pub const DEFAULT_CONTENT_TYPE: &str = "binary/octet-stream";

/// an object's descriptor: everything a `HEAD` answers.
///
/// `etag` is a quoted SHA-256 digest, not the MD5 value that real S3 returns for a single-part
/// upload. Runinator uses SHA-256 for content-addressed data, so callers must not assume MD5
/// semantics here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    /// lowercase hex sha-256 of the full object.
    pub sha256: String,
    pub content_type: String,
    pub last_modified: DateTime<Utc>,
    /// `x-amz-meta-*` headers, with the prefix stripped and names lowercased.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl ObjectMeta {
    /// the quoted entity tag for this object.
    pub fn etag(&self) -> String {
        format!("\"{}\"", self.sha256)
    }
}

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

/// an object's bytes plus the descriptor they came from. a ranged read carries the resolved range so
/// the caller can build a `Content-Range` without re-deriving it.
#[derive(Debug, Clone)]
pub struct ObjectBytes {
    pub meta: ObjectMeta,
    pub range: Option<ResolvedRange>,
    pub data: Vec<u8>,
}

/// lowercase hex sha-256 of a byte slice. the one digest helper every blob caller uses, so it lives
/// beside the descriptor that stores the result.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// the `x-amz-checksum-sha256` wire form of a hex digest.
///
/// S3 sends this header as base64. An AWS SDK decodes it and checks the digest. Emitting hex here
/// would make SDK downloads fail their integrity checks even when the bytes were correct. Runinator
/// stores and displays digests as hex elsewhere, so the conversion lives at the wire boundary
/// rather than in the model.
pub fn sha256_hex_to_base64(hex_digest: &str) -> Option<String> {
    let bytes = hex::decode(hex_digest).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    Some(BASE64.encode(bytes))
}

/// read a checksum header, accepting either the base64 an sdk sends or the hex runinator's own
/// callers use, and normalising to lowercase hex.
pub fn sha256_from_checksum_header(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() == 64 && value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Some(value.to_ascii_lowercase());
    }
    let bytes = BASE64.decode(value).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    Some(hex::encode(bytes))
}
