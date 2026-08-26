//! settings-secret payload and expiry metadata round trips.

use super::*;

#[test]
fn secrets_without_expiry_round_trip_in_a_versioned_envelope() {
    let secret = StoredSecret::new("token".into(), None);
    let encoded = secret.encode().unwrap();

    assert!(encoded.starts_with(b"runinator-secret:v1:"));
    assert_eq!(StoredSecret::decode(&encoded).unwrap(), secret);
}

#[test]
fn expiry_metadata_round_trips_in_a_versioned_envelope() {
    let expires_at = "2026-09-01T12:00:00Z".parse().unwrap();
    let secret = StoredSecret::new("token".into(), Some(expires_at));
    let encoded = secret.encode().unwrap();

    assert_ne!(encoded, b"token");
    assert_eq!(StoredSecret::decode(&encoded).unwrap(), secret);
}

#[test]
fn non_envelope_and_malformed_values_are_rejected() {
    assert!(StoredSecret::decode(b"token").is_err());
    assert!(StoredSecret::decode(b"runinator-secret:v1:not-json").is_err());
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
