// authenticated cipher for settings values at rest. each value is sealed with ChaCha20-Poly1305 and
// tagged with `MAGIC || key_id || nonce`, so a rotated key set decrypts with the matching key: new
// writes use the primary key, and `secondaries` keep pre-rotation values readable during the overlap
// window. values written before authenticated encryption (bare repeating-key xor, no header) are
// still readable through the legacy path.

use chacha20poly1305::aead::{Aead, OsRng};
use chacha20poly1305::{AeadCore, ChaCha20Poly1305, Key, KeyInit, Nonce};
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

/// one keyset entry: the raw key material (for the legacy xor path), the derived 32-byte aead key, and
/// a short stable id used to tag/recognize this key's ciphertext.
#[derive(Clone)]
struct CipherKey {
    raw: Vec<u8>,
    enc_key: [u8; 32],
    id: [u8; KEY_ID_LEN],
}

impl CipherKey {
    fn new(raw: Vec<u8>) -> Self {
        let enc_key = derive(ENC_DOMAIN, &raw);
        let digest = derive(ID_DOMAIN, &raw);
        let mut id = [0u8; KEY_ID_LEN];
        id.copy_from_slice(&digest[..KEY_ID_LEN]);
        Self { raw, enc_key, id }
    }

    fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    fn aead(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new(Key::from_slice(&self.enc_key))
    }
}

/// authenticated cipher with key rotation: seals with the primary key and opens with whichever
/// configured key (primary or a secondary) sealed the stored value.
#[derive(Clone)]
pub struct SecretCipher {
    primary: CipherKey,
    secondaries: Vec<CipherKey>,
}

impl SecretCipher {
    /// build a cipher from a single key; an empty key disables encryption (identity transform).
    pub fn new(key: impl AsRef<[u8]>) -> Self {
        Self {
            primary: CipherKey::new(key.as_ref().to_vec()),
            secondaries: Vec::new(),
        }
    }

    /// build a cipher that seals with `primary` but can still open values sealed by any of the
    /// decrypt-only `secondaries` (the pre-rotation keys kept readable during the overlap window).
    pub fn with_secondaries<P, S, I>(primary: P, secondaries: I) -> Self
    where
        P: AsRef<[u8]>,
        S: AsRef<[u8]>,
        I: IntoIterator<Item = S>,
    {
        Self {
            primary: CipherKey::new(primary.as_ref().to_vec()),
            secondaries: secondaries
                .into_iter()
                .map(|key| key.as_ref().to_vec())
                .filter(|bytes| !bytes.is_empty())
                .map(CipherKey::new)
                .collect(),
        }
    }

    /// build a cipher from the environment: `RUNINATOR_CREDENTIAL_KEY` is the primary, and the
    /// optional comma-separated `RUNINATOR_CREDENTIAL_KEY_PREVIOUS` lists decrypt-only prior keys.
    pub fn from_env() -> Self {
        let primary = std::env::var("RUNINATOR_CREDENTIAL_KEY").unwrap_or_else(|_| DEV_KEY.into());
        let previous = std::env::var("RUNINATOR_CREDENTIAL_KEY_PREVIOUS").unwrap_or_default();
        let secondaries = previous
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| part.to_string());
        Self::with_secondaries(primary, secondaries)
    }

    /// seal plaintext for storage, tagging it with the primary key id and a fresh random nonce.
    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        if self.primary.is_empty() {
            return plaintext.to_vec();
        }
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        // ChaCha20-Poly1305 only fails on absurdly large inputs; settings values are tiny.
        let sealed = self
            .primary
            .aead()
            .encrypt(&nonce, plaintext)
            .expect("aead sealing of a settings value cannot fail");
        let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.primary.id);
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&sealed);
        out
    }

    /// open stored ciphertext, returning `None` only when an authenticated value cannot be opened by
    /// any configured key (a wrong, missing, or retired key, or tampering). legacy headerless values
    /// are recovered with the primary key and always return `Some`.
    pub fn try_decrypt(&self, value: &[u8]) -> Option<Vec<u8>> {
        let Some((id, nonce, body)) = parse_sealed(value) else {
            // legacy value written before authenticated encryption: bare xor with the primary key.
            return Some(self.legacy_decrypt(value));
        };
        let nonce = Nonce::from_slice(nonce);
        // prefer the key named by the tag, then fall back to the rest; the auth tag gates correctness.
        if let Some(key) = self.find_key(id) {
            if let Ok(plaintext) = key.aead().decrypt(nonce, body) {
                return Some(plaintext);
            }
        }
        self.keys()
            .filter(|key| !key.is_empty())
            .find_map(|key| key.aead().decrypt(nonce, body).ok())
    }

    /// open stored ciphertext, yielding empty bytes for an unrecoverable authenticated value. prefer
    /// [`Self::try_decrypt`] where a decrypt failure must be handled.
    pub fn decrypt(&self, value: &[u8]) -> Vec<u8> {
        self.try_decrypt(value).unwrap_or_default()
    }

    /// whether a stored value should be re-sealed with the current primary key: true for legacy
    /// headerless values and for values sealed by a non-primary key. always false in identity mode.
    pub fn needs_reencrypt(&self, value: &[u8]) -> bool {
        if self.primary.is_empty() {
            return false;
        }
        match parse_sealed(value) {
            Some((id, _, _)) => id != self.primary.id,
            None => true,
        }
    }

    // bare repeating-key xor with the primary key, preserving identity for an empty key.
    fn legacy_decrypt(&self, input: &[u8]) -> Vec<u8> {
        if self.primary.is_empty() {
            return input.to_vec();
        }
        xor(input, &self.primary.raw)
    }

    fn keys(&self) -> impl Iterator<Item = &CipherKey> {
        std::iter::once(&self.primary).chain(self.secondaries.iter())
    }

    fn find_key(&self, id: [u8; KEY_ID_LEN]) -> Option<&CipherKey> {
        self.keys().find(|key| !key.is_empty() && key.id == id)
    }
}

impl std::fmt::Debug for SecretCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // never print key material.
        f.debug_struct("SecretCipher")
            .field("keys", &(self.secondaries.len() + 1))
            .finish()
    }
}

// split a sealed value into its key id, nonce, and ciphertext+tag body, or None when headerless.
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

// repeating-key xor, used only to read legacy pre-aead values.
fn xor(input: &[u8], key: &[u8]) -> Vec<u8> {
    input
        .iter()
        .enumerate()
        .map(|(index, byte)| byte ^ key[index % key.len()])
        .collect()
}

#[cfg(test)]
#[path = "secret_cipher_tests.rs"]
mod tests;
