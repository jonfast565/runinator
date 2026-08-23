//! application service for task-run artifact metadata and bytes.

use std::sync::Arc;

use runinator_blob_core::BlobStore;
use runinator_broker_core::{AppEvent, AppEventKind, UiEventPublisher, emit};
use runinator_models::{
    errors::SendableError,
    runs::{NewRunArtifact, RunArtifact},
};
use runinator_store::{RuntimeStore, roles::TaskRunStore};
use uuid::Uuid;

use crate::{artifact_storage::ArtifactContent, repository};

/// Keeps artifact persistence, blob access, and artifact-created events together.
#[derive(Clone)]
pub struct ArtifactOperations<T> {
    store: Arc<T>,
    blobs: Arc<dyn BlobStore>,
    events: UiEventPublisher,
}

impl<T> ArtifactOperations<T> {
    pub fn new(store: Arc<T>, blobs: Arc<dyn BlobStore>, events: UiEventPublisher) -> Self {
        Self {
            store,
            blobs,
            events,
        }
    }
}

impl<T: TaskRunStore + RuntimeStore> ArtifactOperations<T> {
    pub async fn list_for_run(&self, run_id: Uuid) -> Result<Vec<RunArtifact>, SendableError> {
        repository::fetch_run_artifacts(self.store.as_ref(), run_id).await
    }

    pub async fn add(
        &self,
        run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> Result<RunArtifact, SendableError> {
        repository::add_run_artifact(self.store.as_ref(), run_id, artifact).await
    }

    pub async fn list(&self) -> Result<Vec<RunArtifact>, SendableError> {
        repository::fetch_all_artifacts(self.store.as_ref()).await
    }

    pub async fn persist(
        &self,
        run_id: Uuid,
        name: &str,
        mime_type: &str,
        bytes: &[u8],
        fallback_org_id: Option<Uuid>,
    ) -> Result<RunArtifact, SendableError> {
        let artifact = repository::persist_artifact_file(
            self.store.as_ref(),
            &self.blobs,
            run_id,
            name,
            mime_type,
            bytes,
        )
        .await?;
        let org_id = repository::org_id_for_workflow_run(self.store.as_ref(), run_id)
            .await
            .or(fallback_org_id);
        emit(
            &self.events,
            AppEvent::new(
                org_id,
                AppEventKind::ArtifactCreated {
                    artifact_id: artifact.id,
                    run_id: artifact.run_id,
                },
            ),
        );
        Ok(artifact)
    }

    pub async fn delete(&self, artifact_id: Uuid) -> Result<bool, SendableError> {
        repository::delete_artifact(self.store.as_ref(), &self.blobs, artifact_id).await
    }

    pub async fn fetch(&self, artifact_id: Uuid) -> Result<Option<RunArtifact>, SendableError> {
        repository::fetch_artifact(self.store.as_ref(), artifact_id).await
    }

    pub async fn open(&self, uri: &str) -> Result<ArtifactContent, SendableError> {
        crate::artifact_storage::open_artifact(&self.blobs, uri, None).await
    }

    pub async fn put_content(
        &self,
        run_id: Uuid,
        name: &str,
        mime_type: &str,
        bytes: &[u8],
    ) -> Result<String, SendableError> {
        crate::artifact_storage::put_artifact(&self.blobs, run_id, name, mime_type, bytes).await
    }
}
