//! how a deployment describes its blob store.
//!
//! one env vocabulary serves both ends: the service reads it to decide what to serve, and a client
//! (the web service, a worker) reads it to decide what to talk to. that keeps a misconfiguration
//! from producing a server and a client that disagree about the region or the credentials.

use std::env;

use runinator_blob_core::sigv4::{BlobCredential, CredentialStore, DEFAULT_REGION};
use runinator_blob_core::BlobError;

/// where the service listens.
pub const ENV_LISTEN_ADDR: &str = "RUNINATOR_BLOB_ADDR";
/// the directory the service stores objects in.
pub const ENV_DATA_DIR: &str = "RUNINATOR_BLOB_DATA_DIR";
/// the endpoint a client talks to. absent means "use a local directory instead of a service".
pub const ENV_ENDPOINT: &str = "RUNINATOR_BLOB_ENDPOINT";
pub const ENV_ACCESS_KEY_ID: &str = "RUNINATOR_BLOB_ACCESS_KEY_ID";
pub const ENV_SECRET_ACCESS_KEY: &str = "RUNINATOR_BLOB_SECRET_ACCESS_KEY";
/// a json array of `{access_key_id, secret_access_key}` for deployments with more than one key.
pub const ENV_CREDENTIALS: &str = "RUNINATOR_BLOB_CREDENTIALS";
pub const ENV_REGION: &str = "RUNINATOR_BLOB_REGION";
/// accept unsigned requests. development only.
pub const ENV_ALLOW_ANONYMOUS: &str = "RUNINATOR_BLOB_ALLOW_ANONYMOUS";
/// the largest single-part upload the service will buffer, in bytes.
pub const ENV_MAX_OBJECT_BYTES: &str = "RUNINATOR_BLOB_MAX_OBJECT_BYTES";

pub const DEFAULT_LISTEN_ADDR: &str = "0.0.0.0:9000";
pub const DEFAULT_DATA_DIR: &str = "/var/lib/runinator/blobs";
/// 256 MiB. a single-part upload is buffered in memory to verify its digest, so this is a memory
/// bound as much as a size limit; larger objects go through multipart, which buffers one part.
pub const DEFAULT_MAX_OBJECT_BYTES: usize = 256 * 1024 * 1024;

/// the service's runtime configuration.
#[derive(Clone, Debug)]
pub struct BlobServerConfig {
    pub listen_addr: String,
    pub data_dir: String,
    pub region: String,
    pub credentials: CredentialStore,
    pub max_object_bytes: usize,
}

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
            max_object_bytes: env::var(ENV_MAX_OBJECT_BYTES)
                .ok()
                .and_then(|raw| raw.parse().ok())
                .unwrap_or(DEFAULT_MAX_OBJECT_BYTES),
        })
    }
}

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

/// the credentials the service will accept.
pub fn credential_store_from_env() -> Result<CredentialStore, BlobError> {
    let mut credentials = Vec::new();
    if let Ok(raw) = env::var(ENV_CREDENTIALS) {
        if !raw.trim().is_empty() {
            let parsed: Vec<BlobCredential> = serde_json::from_str(&raw).map_err(|err| {
                BlobError::BadRequest(format!(
                    "{ENV_CREDENTIALS} is not a valid credential list: {err}"
                ))
            })?;
            credentials.extend(parsed);
        }
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
    let access_key_id = env::var(ENV_ACCESS_KEY_ID).ok().filter(|v| !v.is_empty())?;
    let secret_access_key = env::var(ENV_SECRET_ACCESS_KEY)
        .ok()
        .filter(|v| !v.is_empty())?;
    Some(BlobCredential {
        access_key_id,
        secret_access_key,
    })
}

fn env_or(name: &str, fallback: &str) -> String {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes"
            )
        })
        .unwrap_or(false)
}
