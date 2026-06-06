// symmetric cipher for settings values at rest. a repeating-key xor keyed by
// `RUNINATOR_CREDENTIAL_KEY`; an empty key is identity. the same routine encrypts and decrypts, so
// callers store the output of `encrypt` and recover plaintext with `decrypt`.

/// repeating-key xor cipher for encrypting stored setting values.
#[derive(Debug, Clone)]
pub struct SecretCipher {
    key: Vec<u8>,
}

impl SecretCipher {
    /// build a cipher from a key; an empty key disables encryption (identity transform).
    pub fn new(key: impl AsRef<[u8]>) -> Self {
        Self {
            key: key.as_ref().to_vec(),
        }
    }

    // xor each byte with the repeating key; identity when the key is empty.
    fn crypt(&self, input: &[u8]) -> Vec<u8> {
        if self.key.is_empty() {
            return input.to_vec();
        }
        input
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ self.key[index % self.key.len()])
            .collect()
    }

    /// encrypt plaintext bytes for storage.
    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        self.crypt(plaintext)
    }

    /// decrypt stored ciphertext bytes.
    pub fn decrypt(&self, ciphertext: &[u8]) -> Vec<u8> {
        self.crypt(ciphertext)
    }
}

#[cfg(test)]
#[path = "secret_cipher_tests.rs"]
mod tests;
