//! moving provider-produced artifact bytes off the worker.
//!
//! a provider writes its artifacts into the local `artifact_dir` the worker handed it and reports
//! the path. that path means nothing to the web service — it names a directory on this worker — so
//! before the artifact event is published the bytes are uploaded and the uri rewritten to the
//! `blob://` form every replica can read.
//!
//! failure is not fatal: the local path is reported as before, which is exactly today's behavior.
//! an artifact readable only from this worker is worse than one readable from anywhere, but it is
//! much better than failing a node that already did its work.

use std::path::Path;
use std::sync::Arc;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_comm::ActionCommand;
use runinator_models::runs::NewRunArtifact;
use tracing::{debug, warn};

/// the largest artifact this will read into memory to upload. beyond it the local path is kept, on
/// the grounds that a worker quietly buffering a multi-gigabyte file is a worse failure than an
/// artifact that stays where the provider wrote it.
pub const MAX_UPLOAD_BYTES: u64 = 256 * 1024 * 1024;

/// uploads artifact bytes to the web service so they outlive this worker.
pub struct ArtifactUploader {
    client: AsyncApiClient<StaticLocator>,
}

impl ArtifactUploader {
    pub fn new(client: AsyncApiClient<StaticLocator>) -> Arc<Self> {
        Arc::new(Self { client })
    }

    /// rewrite an artifact's `uri` to a durable one, or leave it alone if that is not possible.
    pub async fn relocate(&self, command: &ActionCommand, artifact: &mut NewRunArtifact) {
        let path = Path::new(&artifact.uri);
        // a uri that is not a local file is already durable (or is a reference the provider chose);
        // either way it is not ours to rewrite.
        if !path.is_absolute() {
            return;
        }
        let metadata = match tokio::fs::metadata(path).await {
            Ok(metadata) if metadata.is_file() => metadata,
            _ => return,
        };
        if metadata.len() > MAX_UPLOAD_BYTES {
            warn!(
                node_run_id = %command.workflow_node_run_id,
                bytes = metadata.len(),
                "artifact is larger than the upload limit; leaving it on this worker"
            );
            return;
        }
        let bytes = match tokio::fs::read(path).await {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(
                    node_run_id = %command.workflow_node_run_id,
                    "could not read artifact {}: {err}", artifact.uri
                );
                return;
            }
        };
        let size_bytes = bytes.len() as i64;
        match self
            .client
            .upload_artifact_content(
                command.workflow_run_id,
                Some(command.workflow_node_run_id),
                &artifact.name,
                &artifact.mime_type,
                bytes,
            )
            .await
        {
            Ok(stored) => {
                debug!(
                    node_run_id = %command.workflow_node_run_id,
                    "relocated artifact {} to {}", artifact.name, stored.uri
                );
                artifact.uri = stored.uri;
                // the provider's reported size is advisory; the bytes actually stored are not.
                artifact.size_bytes = size_bytes;
            }
            Err(err) => {
                warn!(
                    node_run_id = %command.workflow_node_run_id,
                    "artifact upload failed; keeping the worker-local path: {err}"
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "artifact_upload_tests.rs"]
mod tests;
