use super::*;

#[test]
fn hashes_bytes_and_renders_the_canonical_digest() {
    assert_eq!(sha256(b"hello").len(), 32);
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    assert_eq!(
        sha256_digest(b"hello"),
        digest_from_hex(&sha256_hex(b"hello"))
    );
}

#[test]
fn parses_and_validates_only_prefixed_sha256_values() {
    let digest = sha256_digest(b"hello");
    assert_eq!(parse_digest(&digest).unwrap().len(), 32);
    assert_eq!(hex_part(&digest), sha256_hex(b"hello"));
    assert!(is_valid_digest(&digest));
    assert!(!is_valid_digest(hex_part(&digest)));
    assert!(is_valid_hex(hex_part(&digest)));
    assert!(is_valid_lowercase_hex(hex_part(&digest)));
    assert!(!is_valid_lowercase_hex(
        &hex_part(&digest).to_ascii_uppercase()
    ));
    assert!(!is_valid_digest("sha256:short"));
}
