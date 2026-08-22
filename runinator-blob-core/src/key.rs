//! a validated object key.
//!
//! keys are mirrored onto the filesystem by the local backend, so validation is the whole defence
//! against path traversal. it rejects rather than sanitizes: a caller that sends `a/../../etc` has a
//! bug, and silently rewriting it to something that "works" hides the bug and invents a key nobody
//! asked for.

use std::fmt;

use crate::errors::BlobError;

/// Maximum key length accepted by S3, in UTF-8 bytes.
/// Keep it the same here so a key also works after switching to real S3.
pub const MAX_KEY_BYTES: usize = 1024;

/// an object key that is safe to join onto a directory root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey(String);

impl ObjectKey {
    /// validate a raw key. accepts the S3 "safe characters" set plus `/` as a separator.
    pub fn parse(raw: &str) -> Result<Self, BlobError> {
        if raw.is_empty() {
            return Err(BlobError::InvalidKey("key is empty".into()));
        }
        if raw.len() > MAX_KEY_BYTES {
            return Err(BlobError::InvalidKey(format!(
                "key is {} bytes, limit is {MAX_KEY_BYTES}",
                raw.len()
            )));
        }
        if raw.starts_with('/') || raw.ends_with('/') {
            return Err(BlobError::InvalidKey(
                "key may not start or end with '/'".into(),
            ));
        }
        for segment in raw.split('/') {
            if segment.is_empty() {
                return Err(BlobError::InvalidKey(
                    "key has an empty path segment".into(),
                ));
            }
            if segment == "." || segment == ".." {
                return Err(BlobError::InvalidKey(format!(
                    "key has a relative path segment '{segment}'"
                )));
            }
        }
        if let Some(bad) = raw.chars().find(|character| !is_safe(*character)) {
            return Err(BlobError::InvalidKey(format!(
                "key contains unsupported character {bad:?}"
            )));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// the S3 safe-character set, plus `/`.
fn is_safe(character: char) -> bool {
    character.is_ascii_alphanumeric() || "!-_.*'()/".contains(character)
}

/// validate a bucket name. dns-compatible lowercase, which is what path-style and virtual-host
/// addressing agree on.
pub fn validate_bucket(name: &str) -> Result<(), BlobError> {
    if !(3..=63).contains(&name.len()) {
        return Err(BlobError::BadRequest(format!(
            "bucket name '{name}' must be 3-63 characters"
        )));
    }
    if name.starts_with('-') || name.ends_with('-') || name.starts_with('.') || name.ends_with('.')
    {
        return Err(BlobError::BadRequest(format!(
            "bucket name '{name}' may not start or end with '-' or '.'"
        )));
    }
    if name.contains("..") {
        return Err(BlobError::BadRequest(format!(
            "bucket name '{name}' may not contain '..'"
        )));
    }
    let valid = name.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'
            || character == '.'
    });
    if !valid {
        return Err(BlobError::BadRequest(format!(
            "bucket name '{name}' must be lowercase alphanumeric, '-', or '.'"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "key_tests.rs"]
mod key_tests;
