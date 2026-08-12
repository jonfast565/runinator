//! enrollment TLS identity and pin parsing.

use super::*;

#[test]
fn spki_pin_accepts_standard_and_prefixed_base64() {
    let bytes = [7_u8; 32];
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    assert_eq!(decode_spki_pin(&encoded).unwrap(), bytes);
    assert_eq!(
        decode_spki_pin(&format!("sha256/{encoded}")).unwrap(),
        bytes
    );
}

#[test]
fn spki_pin_rejects_malformed_or_wrong_length_values() {
    assert!(decode_spki_pin("not base64").is_err());
    assert!(
        decode_spki_pin(&base64::engine::general_purpose::STANDARD.encode([0_u8; 31])).is_err()
    );
}
