//! Application service for staged and reusable workflow input files.
//!
//! File bytes are written exactly once to the shared blob store. The store role records the
//! authorization and lifecycle metadata that lets the VM retain a portable descriptor instead of
//! a legacy run-artifact row.

use std::sync::Arc;

use chrono::Utc;
use runinator_blob_core::{
    BlobStore, ObjectKey, PutOptions, WORKFLOW_FILE_BUCKET, blob_uri, sha256_hex,
};
use runinator_models::{
    errors::SendableError,
    files::{FileDescriptor, FileScope, StoredFile, validate_relative_path},
};
use runinator_store::roles::FileStore;
use uuid::Uuid;

#[derive(Clone)]
pub struct WorkflowFiles<T> {
    store: Arc<T>,
    blobs: Arc<dyn BlobStore>,
}

impl<T> WorkflowFiles<T> {
    pub fn new(store: Arc<T>, blobs: Arc<dyn BlobStore>) -> Self {
        Self { store, blobs }
    }

    /// Open a VM effect artifact without exposing the run-artifact storage implementation to the
    /// HTTP layer. Authorization remains the caller's responsibility because the URI is already
    /// reached through an authorized effect-output record.
    pub async fn open_artifact_uri(
        &self,
        uri: &str,
    ) -> Result<crate::artifact_storage::ArtifactContent, SendableError> {
        crate::artifact_storage::open_artifact(&self.blobs, uri, None).await
    }

    pub async fn put_artifact(
        &self,
        run_id: Uuid,
        name: &str,
        mime_type: &str,
        bytes: &[u8],
    ) -> Result<String, SendableError> {
        crate::artifact_storage::put_artifact(&self.blobs, run_id, name, mime_type, bytes).await
    }
}

impl<T: FileStore> WorkflowFiles<T> {
    pub async fn stage(
        &self,
        org_id: Option<Uuid>,
        owner_id: Option<Uuid>,
        path: String,
        mime_type: String,
        bytes: Vec<u8>,
    ) -> Result<StoredFile, SendableError> {
        self.store_file(
            FileScope::Staged,
            org_id,
            owner_id,
            path,
            mime_type,
            bytes,
            1,
        )
        .await
    }

    pub async fn publish_library(
        &self,
        org_id: Option<Uuid>,
        owner_id: Option<Uuid>,
        path: String,
        mime_type: String,
        bytes: Vec<u8>,
    ) -> Result<StoredFile, SendableError> {
        let revision = self.store.next_library_revision(org_id, &path).await?;
        self.store_file(
            FileScope::Library,
            org_id,
            owner_id,
            path,
            mime_type,
            bytes,
            revision,
        )
        .await
    }

    pub async fn list_library(
        &self,
        org_id: Option<Uuid>,
    ) -> Result<Vec<StoredFile>, SendableError> {
        self.store.list_library_files(org_id).await
    }

    pub async fn fetch(&self, id: Uuid) -> Result<Option<StoredFile>, SendableError> {
        self.store.fetch_file(id).await
    }

    pub async fn archive(&self, id: Uuid, org_id: Option<Uuid>) -> Result<bool, SendableError> {
        self.store.archive_file(id, org_id).await
    }

    pub async fn claim_staged(
        &self,
        ids: &[Uuid],
        org_id: Option<Uuid>,
        owner_id: Option<Uuid>,
        workflow_run_id: Uuid,
    ) -> Result<Vec<StoredFile>, SendableError> {
        self.store
            .claim_staged_files(ids, org_id, owner_id, workflow_run_id)
            .await
    }

    pub async fn open(
        &self,
        file: &StoredFile,
    ) -> Result<runinator_engine_file_content::Content, SendableError> {
        let (bucket, key) = runinator_blob_core::parse_blob_uri(&file.uri).ok_or_else(|| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid workflow file URI",
            )) as SendableError
        })?;
        let reader = self
            .blobs
            .open(&bucket, &key, None)
            .await
            .map_err(|error| Box::new(std::io::Error::other(error)) as SendableError)?;
        Ok(runinator_engine_file_content::Content {
            size_bytes: reader.len(),
            body: reader.body,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "storage validates the independent scope, ownership, revision, metadata, and bytes supplied by callers"
    )]
    async fn store_file(
        &self,
        scope: FileScope,
        org_id: Option<Uuid>,
        owner_id: Option<Uuid>,
        path: String,
        mime_type: String,
        bytes: Vec<u8>,
        revision: i64,
    ) -> Result<StoredFile, SendableError> {
        validate_relative_path(&path).map_err(|message| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                message,
            )) as SendableError
        })?;
        let id = Uuid::now_v7();
        let name = path.rsplit('/').next().unwrap_or("file").to_string();
        let key =
            ObjectKey::parse(&format!("{}/{}/{}", scope.as_str(), id, path)).map_err(|error| {
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
                    as SendableError
            })?;
        self.blobs
            .put(
                WORKFLOW_FILE_BUCKET,
                &key,
                bytes.clone(),
                PutOptions {
                    content_type: Some(mime_type.clone()),
                    ..PutOptions::default()
                },
            )
            .await
            .map_err(|error| Box::new(std::io::Error::other(error)) as SendableError)?;
        let file = StoredFile {
            descriptor: FileDescriptor {
                id,
                name,
                path,
                mime_type,
                size_bytes: bytes.len() as i64,
                sha256: sha256_hex(&bytes),
            },
            scope,
            org_id,
            owner_id,
            workflow_run_id: None,
            uri: blob_uri(WORKFLOW_FILE_BUCKET, &key),
            revision,
            current: true,
            archived: false,
            created_at: Utc::now(),
        };
        self.store.insert_file(&file).await
    }
}

/// Kept private to this service so the handler can stream without importing the generic artifact
/// storage module (whose run-artifact semantics should remain separate).
pub mod runinator_engine_file_content {
    pub struct Content {
        pub size_bytes: u64,
        pub body: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    }
}
