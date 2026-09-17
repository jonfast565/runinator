#[allow(unused_imports)]
use super::*;

/// the credentials a server will accept, keyed by access key id.
#[derive(Clone, Debug, Default)]
pub struct CredentialStore {
    pub(super) keys: BTreeMap<String, String>,
    /// when true, an unsigned request is accepted. for local development only.
    pub(super) allow_anonymous: bool,
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
