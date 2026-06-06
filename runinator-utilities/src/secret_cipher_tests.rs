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
