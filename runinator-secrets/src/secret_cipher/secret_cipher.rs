#[allow(unused_imports)]
use super::*;

/// authenticated cipher with key rotation: seals with the primary key and opens with whichever
/// configured key (primary or a secondary) sealed the stored value.
#[derive(Clone)]
pub struct SecretCipher {
    pub(super) primary: CipherKey,
    pub(super) secondaries: Vec<CipherKey>,
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
        let nonce = Nonce::generate();
        // chacha20-poly1305 only fails on absurdly large inputs; settings values are tiny.
        let sealed = self
            .primary
            .aead()
            .encrypt(&nonce, plaintext)
            .expect("aead sealing of a settings value cannot fail");
        let mut out = Vec::with_capacity(HEADER_LEN + sealed.len());
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&self.primary.id);
        out.extend_from_slice(nonce.as_ref());
        out.extend_from_slice(&sealed);
        out
    }

    /// Open a current stored ciphertext. Headerless values are accepted only in identity mode;
    /// otherwise they are not a supported persisted-secret format.
    pub fn try_decrypt(&self, value: &[u8]) -> Option<Vec<u8>> {
        let Some((id, nonce, body)) = parse_sealed(value) else {
            return self.primary.is_empty().then(|| value.to_vec());
        };
        let nonce = Nonce::try_from(nonce).ok()?;
        // prefer the key named by the tag, then fall back to the rest; the auth tag gates correctness.
        if let Some(key) = self.find_key(id)
            && let Ok(plaintext) = key.aead().decrypt(&nonce, body)
        {
            return Some(plaintext);
        }
        self.keys()
            .filter(|key| !key.is_empty())
            .find_map(|key| key.aead().decrypt(&nonce, body).ok())
    }

    /// open stored ciphertext, yielding empty bytes for an unrecoverable authenticated value. prefer
    /// [`Self::try_decrypt`] where a decrypt failure must be handled.
    pub fn decrypt(&self, value: &[u8]) -> Vec<u8> {
        self.try_decrypt(value).unwrap_or_default()
    }

    /// Whether a sealed value should be re-sealed with the current primary key. Always false in
    /// identity mode and for unsupported headerless values.
    pub fn needs_reencrypt(&self, value: &[u8]) -> bool {
        if self.primary.is_empty() {
            return false;
        }
        match parse_sealed(value) {
            Some((id, _, _)) => id != self.primary.id,
            None => false,
        }
    }

    pub(super) fn keys(&self) -> impl Iterator<Item = &CipherKey> {
        std::iter::once(&self.primary).chain(self.secondaries.iter())
    }

    pub(super) fn find_key(&self, id: [u8; KEY_ID_LEN]) -> Option<&CipherKey> {
        self.keys().find(|key| !key.is_empty() && key.id == id)
    }

    /// Whether `value` carries this cipher family's authenticated-encryption header.
    pub fn is_sealed(value: &[u8]) -> bool {
        parse_sealed(value).is_some()
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
