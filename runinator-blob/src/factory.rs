//! choosing a blob store from configuration.
//!
//! one function is the whole seam between "this deployment runs a blob service" and "this deployment
//! keeps blobs on a local disk". a caller receives `Arc<dyn BlobStore>` either way and never branches
//! on which it got — the WS artifact handlers, the engine's function-artifact helper, and the
//! desktop agent all read the same trait.

use std::sync::Arc;

use runinator_blob_core::{BlobError, BlobStore, FsBlobStore};

use crate::client::S3BlobClient;
use crate::config::{BlobClientConfig, DEFAULT_DATA_DIR};

/// build the store this process should use.
///
/// with `RUNINATOR_BLOB_ENDPOINT` set, that is the blob service. without it, a local directory —
/// which is what keeps the supervisor stack, the desktop agent, and a single-node install working
/// with no extra process to run.
pub async fn from_env() -> Result<Arc<dyn BlobStore>, BlobError> {
    match BlobClientConfig::from_env() {
        Some(config) => {
            tracing::info!(endpoint = %config.endpoint, "using the blob service for object storage");
            Ok(Arc::new(S3BlobClient::new(config)?))
        }
        None => {
            let dir = local_data_dir();
            tracing::info!(dir = %dir, "no blob endpoint configured; using local object storage");
            Ok(Arc::new(FsBlobStore::open(&dir).await?))
        }
    }
}

/// ensure the buckets runinator writes to exist. safe to call on every boot: bucket creation is
/// idempotent, and doing it here means no deployment has a "remember to create the bucket" step.
pub async fn ensure_buckets(store: &Arc<dyn BlobStore>) -> Result<(), BlobError> {
    for bucket in [
        runinator_blob_core::FUNCTION_ARTIFACT_BUCKET,
        runinator_blob_core::RUN_ARTIFACT_BUCKET,
    ] {
        store.create_bucket(bucket).await?;
    }
    Ok(())
}

/// where a local store keeps its data when no service is configured. under the app data directory
/// rather than the service's `/var/lib` default, since this path is a workstation or a single-node
/// install rather than a container.
fn local_data_dir() -> String {
    if let Ok(configured) = std::env::var(crate::config::ENV_DATA_DIR) {
        if !configured.is_empty() {
            return configured;
        }
    }
    runinator_platform::app_data::app_data_path("blobs")
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| DEFAULT_DATA_DIR.to_string())
}
