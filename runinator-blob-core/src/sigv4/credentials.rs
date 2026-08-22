//! the static credentials both ends sign with.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::BlobError;

/// the default region token. nothing here is region-aware, but sigv4's credential scope requires
/// one, and a mismatch between client and server is a signature failure, so it is pinned.
pub const DEFAULT_REGION: &str = "us-east-1";

/// the service token in the credential scope. `S3` so an unmodified aws sdk signs correctly.
pub const SERVICE: &str = "s3";

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

/// the credentials a server will accept, keyed by access key id.
#[derive(Clone, Debug, Default)]
pub struct CredentialStore {
    keys: BTreeMap<String, String>,
    /// when true, an unsigned request is accepted. for local development only.
    allow_anonymous: bool,
}

impl CredentialStore {
    pub fn new(credentials: impl IntoIterator<Item = BlobCredential>) -> Self {
        Self {
            keys: credentials
                .into_iter()
                .map(|credential| (credential.access_key_id, credential.secret_access_key))
                .collect(),
            allow_anonymous: false,
        }
    }

    /// accept unsigned requests. the supervisor stack runs this way so local development needs no
    /// key material; never enable it on a reachable deployment.
    pub fn allowing_anonymous(mut self) -> Self {
        self.allow_anonymous = true;
        self
    }

    pub fn allows_anonymous(&self) -> bool {
        self.allow_anonymous
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn secret_for(&self, access_key_id: &str) -> Result<&str, BlobError> {
        self.keys
            .get(access_key_id)
            .map(String::as_str)
            .ok_or_else(|| BlobError::Unauthorized(format!("unknown access key '{access_key_id}'")))
    }
}
