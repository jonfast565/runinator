#[allow(unused_imports)]
use super::*;

/// persistence-facing API key record: metadata plus the secret hash used to verify a presented key.
#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    pub key: ApiKey,
    pub key_hash: String,
}
