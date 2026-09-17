//! what the store knows about an object besides its bytes.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::range::ResolvedRange;

/// Default content type for an object that has none, matching S3.
pub const DEFAULT_CONTENT_TYPE: &str = "binary/octet-stream";

/// lowercase hex sha-256 of a byte slice. the one digest helper every blob caller uses, so it lives
/// beside the descriptor that stores the result.
pub fn sha256_hex(bytes: &[u8]) -> String {
    runinator_hash::sha256_hex(bytes)
}

/// the `x-amz-checksum-sha256` wire form of a hex digest.
///
/// S3 sends this header as base64. An AWS SDK decodes it and checks the digest. Emitting hex here
/// would make SDK downloads fail their integrity checks even when the bytes were correct. Runinator
/// stores and displays digests as hex elsewhere, so the conversion lives at the wire boundary
/// rather than in the model.
pub fn sha256_hex_to_base64(hex_digest: &str) -> Option<String> {
    let bytes = runinator_hash::parse_hex(hex_digest)?;
    Some(BASE64.encode(bytes))
}

/// read a checksum header, accepting either the base64 an sdk sends or the hex runinator's own
/// callers use, and normalising to lowercase hex.
pub fn sha256_from_checksum_header(value: &str) -> Option<String> {
    let value = value.trim();
    if runinator_hash::is_valid_hex(value) {
        return Some(value.to_ascii_lowercase());
    }
    let bytes = BASE64.decode(value).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    Some(hex::encode(bytes))
}

mod object_meta;
pub use object_meta::ObjectMeta;

mod put_options;
pub use put_options::PutOptions;

mod object_bytes;
pub use object_bytes::ObjectBytes;
