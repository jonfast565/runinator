// authenticated cipher for settings values at rest. each value is sealed with ChaCha20-Poly1305 and
// tagged with `MAGIC || key_id || nonce`, so a rotated key set decrypts with the matching key: new
// writes use the primary key, and `secondaries` keep pre-rotation values readable during the overlap
// window.

use chacha20poly1305::aead::{Aead, Generate};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use sha2::{Digest, Sha256};

const MAGIC: [u8; 4] = [0x52, 0x41, 0x45, 0x31]; // "RAE1" tags an authenticated, key-tagged value.
const KEY_ID_LEN: usize = 4;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = MAGIC.len() + KEY_ID_LEN + NONCE_LEN;

/// default credential key for local development when `RUNINATOR_CREDENTIAL_KEY` is unset.
const DEV_KEY: &str = "runinator-local-development-key";

// domain separators so the persisted key id reveals nothing about the encryption key.
const ENC_DOMAIN: &[u8] = b"runinator/cred/enc\0";
const ID_DOMAIN: &[u8] = b"runinator/cred/id\0";

// split a sealed value into its key id, nonce, and ciphertext+tag body.
fn parse_sealed(value: &[u8]) -> Option<([u8; KEY_ID_LEN], &[u8], &[u8])> {
    if value.len() < HEADER_LEN || value[..MAGIC.len()] != MAGIC {
        return None;
    }
    let mut id = [0u8; KEY_ID_LEN];
    let id_end = MAGIC.len() + KEY_ID_LEN;
    id.copy_from_slice(&value[MAGIC.len()..id_end]);
    let nonce = &value[id_end..HEADER_LEN];
    Some((id, nonce, &value[HEADER_LEN..]))
}

// derive a 32-byte value from a domain separator and the raw key material.
fn derive(domain: &[u8], raw: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(raw);
    hasher.finalize().into()
}

#[cfg(test)]
#[path = "secret_cipher_tests.rs"]
mod tests;

mod cipher_key;
use cipher_key::CipherKey;

mod secret_cipher;
pub use secret_cipher::SecretCipher;
