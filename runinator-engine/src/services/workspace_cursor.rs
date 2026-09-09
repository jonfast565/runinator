//! Compact, bounded transport for immutable browsing cursors.
use base64::Engine;
use runinator_models::errors::{SendableError, WORKSPACE_INVALID};
pub(super) fn encode<T: serde::Serialize>(cursor: &T) -> Result<String, SendableError> {
    let json = serde_json::to_vec(cursor)?;
    if json.len() > 32768 {
        return Err(WORKSPACE_INVALID.error("cursor exceeds path budget"));
    }
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(zstd::bulk::compress(&json, 1)?))
}
pub(super) fn decode<T: serde::de::DeserializeOwned>(cursor: &str) -> Result<T, SendableError> {
    if cursor.len() > 16384 {
        return Err(WORKSPACE_INVALID.error("cursor too large"));
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| WORKSPACE_INVALID.error("invalid cursor"))?;
    let json = zstd::bulk::decompress(&bytes, 32768)
        .map_err(|_| WORKSPACE_INVALID.error("invalid cursor encoding"))?;
    serde_json::from_slice(&json).map_err(|_| WORKSPACE_INVALID.error("invalid cursor state"))
}
