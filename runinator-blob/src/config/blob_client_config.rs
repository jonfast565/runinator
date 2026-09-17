#[allow(unused_imports)]
use super::*;

/// what a client needs to reach a blob service.
#[derive(Clone, Debug)]
pub struct BlobClientConfig {
    pub endpoint: String,
    pub region: String,
    pub credential: Option<BlobCredential>,
}

impl BlobClientConfig {
    /// read the configuration from the environment, or `None` when no endpoint is configured — the
    /// signal that this deployment stores blobs on a local directory instead.
    pub fn from_env() -> Option<Self> {
        let endpoint = env::var(ENV_ENDPOINT)
            .ok()
            .filter(|value| !value.is_empty())?;
        Some(Self {
            endpoint,
            region: env_or(ENV_REGION, DEFAULT_REGION),
            credential: single_credential_from_env(),
        })
    }
}
