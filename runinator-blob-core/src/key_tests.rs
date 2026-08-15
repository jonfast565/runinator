//! covers key and bucket-name validation, especially the traversal rejections.

use super::*;

#[test]
fn accepts_ordinary_keys() {
    assert_eq!(
        ObjectKey::parse("sha256/abc123.zip").unwrap().as_str(),
        "sha256/abc123.zip"
    );
    assert!(ObjectKey::parse("runs/0198-a/report(final).txt").is_ok());
}

#[test]
fn rejects_traversal_and_empty_segments() {
    for raw in ["../etc/passwd", "a/../b", "a/./b", "a//b", "/a", "a/"] {
        assert!(
            matches!(ObjectKey::parse(raw), Err(BlobError::InvalidKey(_))),
            "expected {raw:?} to be rejected"
        );
    }
}

#[test]
fn rejects_empty_oversized_and_unsafe_keys() {
    assert!(ObjectKey::parse("").is_err());
    assert!(ObjectKey::parse(&"a".repeat(MAX_KEY_BYTES + 1)).is_err());
    // a nul byte, a newline, and a backslash are the shapes that break path handling downstream.
    assert!(ObjectKey::parse("a\0b").is_err());
    assert!(ObjectKey::parse("a\nb").is_err());
    assert!(ObjectKey::parse("a\\b").is_err());
}

#[test]
fn validates_bucket_names() {
    assert!(validate_bucket("runinator-function-artifacts").is_ok());
    assert!(validate_bucket("ab").is_err());
    assert!(validate_bucket("-leading").is_err());
    assert!(validate_bucket("trailing-").is_err());
    assert!(validate_bucket("Upper").is_err());
    assert!(validate_bucket("do..ts").is_err());
}
