//! Validation primitives for values that cross a trust boundary.
//!
//! Serde proves that an input has the right shape. [`Validate`] proves the inexpensive semantic
//! invariants that are intrinsic to the type (required text, bounded identifiers, finite ranges,
//! and internally consistent time windows). Database- and policy-dependent checks remain in the
//! service that owns that state.

use std::{collections::BTreeMap, error::Error, fmt};

/// A stable, field-addressable input error suitable for HTTP and UI error surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

impl ValidationError {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(&self.message)
        } else {
            write!(f, "{}: {}", self.path, self.message)
        }
    }
}

impl Error for ValidationError {}

/// Type-owned validation that does not require external state.
pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

pub const SHORT_TEXT_MAX: usize = 256;
pub const LONG_TEXT_MAX: usize = 16 * 1024;
pub const URL_MAX: usize = 2 * 1024;

pub fn required_text(path: &str, value: &str, max: usize) -> Result<(), ValidationError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::new(path, "is required"));
    }
    bounded_text(path, trimmed, max)
}

pub fn optional_text(path: &str, value: Option<&str>, max: usize) -> Result<(), ValidationError> {
    if let Some(value) = value {
        required_text(path, value, max)?;
    }
    Ok(())
}

pub fn bounded_text(path: &str, value: &str, max: usize) -> Result<(), ValidationError> {
    let length = value.chars().count();
    if length > max {
        return Err(ValidationError::new(
            path,
            format!("must be at most {max} characters (received {length})"),
        ));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(ValidationError::new(
            path,
            "must not contain control characters",
        ));
    }
    Ok(())
}

pub fn identifier(path: &str, value: &str) -> Result<(), ValidationError> {
    required_text(path, value, SHORT_TEXT_MAX)?;
    if !value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/' | ':')
    }) {
        return Err(ValidationError::new(
            path,
            "may contain only letters, numbers, '-', '_', '.', '/', and ':'",
        ));
    }
    Ok(())
}

pub fn http_url(path: &str, value: &str) -> Result<(), ValidationError> {
    required_text(path, value, URL_MAX)?;
    let parsed = url::Url::parse(value)
        .map_err(|_| ValidationError::new(path, "must be an absolute http:// or https:// URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(ValidationError::new(
            path,
            "must be an absolute http:// or https:// URL",
        ));
    }
    Ok(())
}

pub fn email(path: &str, value: &str) -> Result<(), ValidationError> {
    required_text(path, value, SHORT_TEXT_MAX)?;
    let Some((local, domain)) = value.split_once('@') else {
        return Err(ValidationError::new(path, "must be a valid email address"));
    };
    if local.is_empty()
        || domain.is_empty()
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains('@')
        || value.chars().any(char::is_whitespace)
    {
        return Err(ValidationError::new(path, "must be a valid email address"));
    }
    Ok(())
}

pub fn optional_email(path: &str, value: Option<&str>) -> Result<(), ValidationError> {
    if let Some(value) = value {
        email(path, value)?;
    }
    Ok(())
}

pub fn string_map(
    path: &str,
    values: &BTreeMap<String, String>,
    max_entries: usize,
) -> Result<(), ValidationError> {
    if values.len() > max_entries {
        return Err(ValidationError::new(
            path,
            format!("must contain at most {max_entries} entries"),
        ));
    }
    for (key, value) in values {
        identifier(&format!("{path}.{key}"), key)?;
        bounded_text(&format!("{path}.{key}"), value, 2 * 1024)?;
    }
    Ok(())
}

pub fn positive_limit(path: &str, value: Option<i64>, max: i64) -> Result<(), ValidationError> {
    if let Some(value) = value
        && !(1..=max).contains(&value)
    {
        return Err(ValidationError::new(
            path,
            format!("must be between 1 and {max}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_text_rejects_whitespace_and_overflow() {
        assert_eq!(
            required_text("name", "  ", 10).unwrap_err().to_string(),
            "name: is required"
        );
        assert!(required_text("name", "eleven chars", 10).is_err());
    }

    #[test]
    fn identifiers_reject_shell_and_space_characters() {
        assert!(identifier("key", "valid/key-1.0").is_ok());
        assert!(identifier("key", "not valid").is_err());
        assert!(identifier("key", "$(unsafe)").is_err());
    }

    #[test]
    fn urls_require_an_absolute_http_scheme() {
        assert!(http_url("url", "https://runinator.example/api").is_ok());
        assert!(http_url("url", "https://").is_err());
        assert!(http_url("url", "file:///tmp/runinator").is_err());
        assert!(http_url("url", "runinator.example").is_err());
    }

    #[test]
    fn emails_require_a_local_and_domain_part() {
        assert!(email("email", "operator@runinator.example").is_ok());
        assert!(email("email", "operator").is_err());
        assert!(email("email", "operator@@runinator.example").is_err());
    }
}
