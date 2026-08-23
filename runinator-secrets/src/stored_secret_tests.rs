//! settings-secret payload compatibility and expiry metadata round trips.

use super::*;

#[test]
fn legacy_payloads_decode_without_expiry() {
    assert_eq!(
        StoredSecret::decode(b"token"),
        StoredSecret::new("token".into(), None)
    );
    assert_eq!(
        StoredSecret::new("token".into(), None).encode().unwrap(),
        b"token"
    );
}

#[test]
fn expiry_metadata_round_trips_in_a_versioned_envelope() {
    let expires_at = "2026-09-01T12:00:00Z".parse().unwrap();
    let secret = StoredSecret::new("token".into(), Some(expires_at));
    let encoded = secret.encode().unwrap();

    assert_ne!(encoded, b"token");
    assert_eq!(StoredSecret::decode(&encoded), secret);
}

#[test]
fn malformed_envelopes_remain_readable_as_legacy_values() {
    let malformed = b"runinator-secret:v1:not-json";
    assert_eq!(
        StoredSecret::decode(malformed),
        StoredSecret::new(String::from_utf8_lossy(malformed).into_owned(), None)
    );
}

#[test]
fn expiry_occurrence_is_stable_and_unambiguous() {
    let expires_at = "2026-09-01T12:00:00Z".parse().unwrap();
    let first = secret_expiry_occurrence("a", "b:c", expires_at, 3_600);
    assert_eq!(
        first,
        secret_expiry_occurrence("a", "b:c", expires_at, 3_600)
    );
    assert_ne!(
        first,
        secret_expiry_occurrence("a:b", "c", expires_at, 3_600)
    );
    assert_eq!(first.len(), 64);
}
