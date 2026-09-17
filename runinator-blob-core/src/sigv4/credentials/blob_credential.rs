#[allow(unused_imports)]
use super::*;

/// one access key pair.
#[derive(Clone, Serialize, Deserialize)]
pub struct BlobCredential {
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl std::fmt::Debug for BlobCredential {
    /// the secret never reaches a log line, including through a derived `Debug` on some struct that
    /// happens to hold one.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BlobCredential")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .finish()
    }
}
