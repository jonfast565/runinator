#[allow(unused_imports)]
use super::*;

/// one keyset entry: the raw key material (to identify identity mode), the derived 32-byte aead
/// key, and a short stable id used to tag/recognize this key's ciphertext.
#[derive(Clone)]
pub(super) struct CipherKey {
    pub(super) raw: Vec<u8>,
    pub(super) enc_key: [u8; 32],
    pub(super) id: [u8; KEY_ID_LEN],
}

impl CipherKey {
    pub(super) fn new(raw: Vec<u8>) -> Self {
        let enc_key = derive(ENC_DOMAIN, &raw);
        let digest = derive(ID_DOMAIN, &raw);
        let mut id = [0u8; KEY_ID_LEN];
        id.copy_from_slice(&digest[..KEY_ID_LEN]);
        Self { raw, enc_key, id }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub(super) fn aead(&self) -> ChaCha20Poly1305 {
        ChaCha20Poly1305::new_from_slice(&self.enc_key)
            .expect("chacha20-poly1305 accepts a 32-byte key")
    }
}
