//! Small, algorithm-explicit hashing and digest helpers shared across storage and content-addressed
//! boundaries.

use sha2::{Digest, Sha256};

pub const SHA256_PREFIX: &str = "sha256:";
pub const SHA256_HEX_LENGTH: usize = 64;

/// computes the raw SHA-256 bytes for a byte slice.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// computes lowercase hexadecimal SHA-256 for a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha256(bytes))
}

/// computes the canonical `sha256:<hex>` representation for a byte slice.
pub fn sha256_digest(bytes: &[u8]) -> String {
    digest_from_hex(&sha256_hex(bytes))
}

/// renders a hexadecimal SHA-256 value with the canonical algorithm prefix.
pub fn digest_from_hex(hex_value: &str) -> String {
    format!("{SHA256_PREFIX}{}", hex_value.to_ascii_lowercase())
}

/// returns the hexadecimal component of a prefixed or unprefixed digest.
pub fn hex_part(digest: &str) -> &str {
    digest.strip_prefix(SHA256_PREFIX).unwrap_or(digest)
}

/// parses an unprefixed 64-character SHA-256 hexadecimal value into raw bytes.
pub fn parse_hex(hex_value: &str) -> Option<[u8; 32]> {
    if hex_value.len() != SHA256_HEX_LENGTH {
        return None;
    }
    hex::decode(hex_value).ok()?.try_into().ok()
}

/// parses a lowercase 64-character SHA-256 hexadecimal value into raw bytes.
pub fn parse_lowercase_hex(hex_value: &str) -> Option<[u8; 32]> {
    if hex_value.len() != SHA256_HEX_LENGTH
        || !hex_value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    parse_hex(hex_value)
}

/// returns whether a value is an unprefixed 64-character SHA-256 hexadecimal value.
pub fn is_valid_hex(hex_value: &str) -> bool {
    parse_hex(hex_value).is_some()
}

/// returns whether a value is a lowercase 64-character SHA-256 hexadecimal value.
pub fn is_valid_lowercase_hex(hex_value: &str) -> bool {
    parse_lowercase_hex(hex_value).is_some()
}

/// parses a canonical prefixed SHA-256 digest into its raw bytes.
pub fn parse_digest(digest: &str) -> Option<[u8; 32]> {
    let hex_value = digest.strip_prefix(SHA256_PREFIX)?;
    parse_hex(hex_value)
}

/// parses a canonical digest whose hexadecimal component is lowercase.
pub fn parse_lowercase_digest(digest: &str) -> Option<[u8; 32]> {
    let hex_value = digest.strip_prefix(SHA256_PREFIX)?;
    parse_lowercase_hex(hex_value)
}

/// returns whether a value is a canonical prefixed SHA-256 digest.
pub fn is_valid_digest(digest: &str) -> bool {
    parse_digest(digest).is_some()
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
