//! how a deployment describes its blob store.
//!
//! one env vocabulary serves both ends: the service reads it to decide what to serve, and a client
//! (the web service, a worker) reads it to decide what to talk to. that keeps a misconfiguration
//! from producing a server and a client that disagree about the region or the credentials.

use runinator_blob_core::sigv4::{BlobCredential, DEFAULT_REGION};
#[cfg(feature = "server")]
use runinator_blob_core::{BlobError, CredentialStore};
use runinator_platform::env as platform_env;

/// where the service listens.
#[cfg(feature = "server")]
pub const ENV_LISTEN_ADDR: &str = "RUNINATOR_BLOB_ADDR";
/// the directory the service stores objects in.
pub const ENV_DATA_DIR: &str = "RUNINATOR_BLOB_DATA_DIR";
/// the endpoint a client talks to. absent means "use a local directory instead of a service".
pub const ENV_ENDPOINT: &str = "RUNINATOR_BLOB_ENDPOINT";
pub const ENV_ACCESS_KEY_ID: &str = "RUNINATOR_BLOB_ACCESS_KEY_ID";
pub const ENV_SECRET_ACCESS_KEY: &str = "RUNINATOR_BLOB_SECRET_ACCESS_KEY";
/// a json array of `{access_key_id, secret_access_key}` for deployments with more than one key.
#[cfg(feature = "server")]
pub const ENV_CREDENTIALS: &str = "RUNINATOR_BLOB_CREDENTIALS";
pub const ENV_REGION: &str = "RUNINATOR_BLOB_REGION";
/// accept unsigned requests. development only.
#[cfg(feature = "server")]
pub const ENV_ALLOW_ANONYMOUS: &str = "RUNINATOR_BLOB_ALLOW_ANONYMOUS";
/// the largest decoded single-part upload the service accepts, in bytes.
#[cfg(feature = "server")]
pub const ENV_MAX_OBJECT_BYTES: &str = "RUNINATOR_BLOB_MAX_OBJECT_BYTES";
/// maximum resident metadata-cache weight for the filesystem backend.
#[cfg(feature = "server")]
pub const ENV_METADATA_CACHE_BYTES: &str = "RUNINATOR_BLOB_METADATA_CACHE_BYTES";
/// maximum filesystem mutations allowed to progress concurrently.
#[cfg(feature = "server")]
pub const ENV_MAX_CONCURRENT_WRITES: &str = "RUNINATOR_BLOB_MAX_CONCURRENT_WRITES";

#[cfg(feature = "server")]
pub const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:9000";
pub const DEFAULT_DATA_DIR: &str = "/var/lib/runinator/blobs";
/// 256 MiB. the service streams this much decoded data into one atomic object or multipart part;
/// larger objects go through multipart.
#[cfg(feature = "server")]
pub const DEFAULT_MAX_OBJECT_BYTES: usize = 256 * 1024 * 1024;

/// the credentials the service will accept.
#[cfg(feature = "server")]
pub fn credential_store_from_env() -> Result<CredentialStore, BlobError> {
    let mut credentials = Vec::new();
    if let Some(raw) = platform_env::non_empty(ENV_CREDENTIALS) {
        let parsed: Vec<BlobCredential> = serde_json::from_str(&raw).map_err(|err| {
            BlobError::BadRequest(format!(
                "{ENV_CREDENTIALS} is not a valid credential list: {err}"
            ))
        })?;
        credentials.extend(parsed);
    }
    if let Some(credential) = single_credential_from_env() {
        credentials.push(credential);
    }
    let store = CredentialStore::new(credentials);
    if env_flag(ENV_ALLOW_ANONYMOUS) {
        return Ok(store.allowing_anonymous());
    }
    Ok(store)
}

fn single_credential_from_env() -> Option<BlobCredential> {
    let access_key_id = platform_env::non_empty(ENV_ACCESS_KEY_ID)?;
    let secret_access_key = platform_env::non_empty(ENV_SECRET_ACCESS_KEY)?;
    Some(BlobCredential {
        access_key_id,
        secret_access_key,
    })
}

fn env_or(name: &str, fallback: &str) -> String {
    platform_env::non_empty(name).unwrap_or_else(|| fallback.to_string())
}

#[cfg(feature = "server")]
fn env_flag(name: &str) -> bool {
    platform_env::flag(name)
}

#[cfg(feature = "server")]
mod blob_server_config;
#[cfg(feature = "server")]
pub use blob_server_config::BlobServerConfig;

mod blob_client_config;
pub use blob_client_config::BlobClientConfig;
