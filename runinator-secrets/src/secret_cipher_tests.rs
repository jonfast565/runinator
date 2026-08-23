use super::SecretCipher;

#[test]
fn round_trips_with_a_key() {
    let cipher = SecretCipher::new("runinator-key");
    let plaintext = b"super-secret-token";
    let ciphertext = cipher.encrypt(plaintext);
    assert_ne!(
        ciphertext, plaintext,
        "ciphertext should differ from plaintext"
    );
    assert_eq!(cipher.decrypt(&ciphertext), plaintext);
}

#[test]
fn empty_key_is_identity() {
    let cipher = SecretCipher::new("");
    let plaintext = b"plain";
    assert_eq!(cipher.encrypt(plaintext), plaintext);
    assert_eq!(cipher.decrypt(plaintext), plaintext);
}

#[test]
fn secondary_key_decrypts_after_rotation() {
    let plaintext = b"super-secret-token";
    // a value written before rotation, tagged with the old key.
    let old = SecretCipher::new("old-key");
    let stored = old.encrypt(plaintext);

    // after rotation the new key is primary; the old key rides along as a secondary.
    let rotated = SecretCipher::with_secondaries("new-key", ["old-key"]);
    assert_eq!(
        rotated.decrypt(&stored),
        plaintext,
        "pre-rotation value must decrypt during the overlap window"
    );

    // new writes are tagged with the primary, so the old-only cipher can no longer recover them.
    let fresh = rotated.encrypt(plaintext);
    assert_ne!(old.decrypt(&fresh), plaintext);
}

#[test]
fn retiring_the_old_key_drops_pre_rotation_values() {
    let plaintext = b"super-secret-token";
    let stored = SecretCipher::new("old-key").encrypt(plaintext);
    // once the secondary is gone the authenticated value can no longer be opened: try_decrypt reports
    // the failure rather than returning garbage.
    let retired = SecretCipher::new("new-key");
    assert_eq!(retired.try_decrypt(&stored), None);
    assert!(retired.decrypt(&stored).is_empty());
}

#[test]
fn tampered_ciphertext_is_rejected() {
    let cipher = SecretCipher::new("runinator-key");
    let mut stored = cipher.encrypt(b"super-secret-token");
    // flip a bit in the ciphertext body; the poly1305 tag must reject it.
    let last = stored.len() - 1;
    stored[last] ^= 0x01;
    assert_eq!(cipher.try_decrypt(&stored), None);
}

#[test]
fn encryption_is_randomized_per_call() {
    let cipher = SecretCipher::new("runinator-key");
    // a fresh nonce per seal means identical plaintext yields distinct ciphertext.
    assert_ne!(cipher.encrypt(b"value"), cipher.encrypt(b"value"));
}

#[test]
fn needs_reencrypt_flags_legacy_and_foreign_keys() {
    let primary = SecretCipher::with_secondaries("new-key", ["old-key"]);
    // sealed by the old (secondary) key -> must be re-sealed with the primary.
    let old_sealed = SecretCipher::new("old-key").encrypt(b"value");
    assert!(primary.needs_reencrypt(&old_sealed));
    // already sealed by the primary key -> left untouched.
    let fresh = primary.encrypt(b"value");
    assert!(!primary.needs_reencrypt(&fresh));
    // a legacy headerless value -> must be re-sealed.
    assert!(primary.needs_reencrypt(b"\x01\x02\x03plain-ish"));
}

#[test]
fn legacy_headerless_value_decrypts_with_primary() {
    let plaintext = b"legacy-secret";
    let key = b"runinator-key";
    // simulate a value written by the pre-rotation cipher: bare repeating-key xor, no header.
    let legacy: Vec<u8> = plaintext
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect();

    let cipher = SecretCipher::new(key);
    assert_eq!(cipher.decrypt(&legacy), plaintext);
}

#[test]
fn encrypt_tags_value_with_primary_key() {
    let cipher = SecretCipher::with_secondaries("primary", ["previous"]);
    let stored = cipher.encrypt(b"value");
    // sealed values start with the aead magic header, and a secondary-only cipher cannot open them.
    assert_eq!(&stored[..4], b"RAE1");
    assert_eq!(SecretCipher::new("previous").try_decrypt(&stored), None);
}
