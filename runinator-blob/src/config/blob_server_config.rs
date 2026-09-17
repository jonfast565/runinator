#[allow(unused_imports)]
use super::*;

/// the service's runtime configuration.
#[cfg(feature = "server")]
#[derive(Clone, Debug)]
pub struct BlobServerConfig {
    pub listen_addr: String,
    pub data_dir: String,
    pub region: String,
    pub credentials: CredentialStore,
    pub max_object_bytes: usize,
    pub metadata_cache_bytes: usize,
    pub max_concurrent_writes: usize,
}

#[cfg(feature = "server")]
impl BlobServerConfig {
    /// read the configuration from the environment.
    pub fn from_env() -> Result<Self, BlobError> {
        let credentials = credential_store_from_env()?;
        if credentials.is_empty() && !credentials.allows_anonymous() {
            return Err(BlobError::BadRequest(format!(
                "no blob credentials configured: set {ENV_ACCESS_KEY_ID}/{ENV_SECRET_ACCESS_KEY}, \
                 or {ENV_CREDENTIALS}, or {ENV_ALLOW_ANONYMOUS}=true for local development"
            )));
        }
        Ok(Self {
            listen_addr: env_or(ENV_LISTEN_ADDR, DEFAULT_LISTEN_ADDR),
            data_dir: env_or(ENV_DATA_DIR, DEFAULT_DATA_DIR),
            region: env_or(ENV_REGION, DEFAULT_REGION),
            credentials,
            max_object_bytes: platform_env::parse_or(
                ENV_MAX_OBJECT_BYTES,
                DEFAULT_MAX_OBJECT_BYTES,
            ),
            metadata_cache_bytes: platform_env::parse_or(
                ENV_METADATA_CACHE_BYTES,
                runinator_blob_core::DEFAULT_METADATA_CACHE_BYTES,
            ),
            max_concurrent_writes: platform_env::parse_or(
                ENV_MAX_CONCURRENT_WRITES,
                runinator_blob_core::DEFAULT_MAX_CONCURRENT_WRITES,
            ),
        })
    }
}
