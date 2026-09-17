//! Durable workspace storage-provider boundary.

use super::{WorkspaceContent, WorkspaceService};
use async_trait::async_trait;
use runinator_models::{errors::SendableError, workspaces::*};
use runinator_store::roles::DurableWorkspaceStore;
use std::sync::{Arc, RwLock};

/// Shared dependencies owned by one concrete workspace storage provider.

/// Current immutable object-graph workspace representation.

impl<T: DurableWorkspaceStore> WorkspaceService<T> {
    pub async fn checkout_content(
        &self,
        checkout: WorkspaceCheckout,
    ) -> Result<WorkspaceContent, SendableError> {
        self.storage.checkout_content(checkout).await
    }

    pub async fn object(
        &self,
        workspace: uuid::Uuid,
        id: String,
        version: i64,
    ) -> Result<Vec<u8>, SendableError> {
        self.storage.object(workspace, id, version).await
    }

    pub async fn upload_pack(&self, id: uuid::Uuid, bytes: Vec<u8>) -> Result<(), SendableError> {
        self.storage.stage_checkout(id, bytes).await
    }

    pub async fn seal(
        &self,
        id: uuid::Uuid,
        request: WorkspaceSeal,
    ) -> Result<WorkspaceReceipt, SendableError> {
        self.storage.seal_checkout(id, request).await
    }

    pub async fn directory(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        self.storage
            .directory(id, version, path, after, limit)
            .await
    }

    pub async fn results(
        &self,
        id: uuid::Uuid,
        version: i64,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        self.storage.results(id, version, after, limit).await
    }

    pub async fn result_content(
        &self,
        id: uuid::Uuid,
        version: i64,
        name: String,
        preview: bool,
    ) -> Result<WorkspaceContent, SendableError> {
        self.storage
            .result_content(id, version, name, preview)
            .await
    }

    pub async fn file_range(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, SendableError> {
        self.storage
            .file_range(id, version, path, offset, length)
            .await
    }

    pub async fn file(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
    ) -> Result<WorkspaceContent, SendableError> {
        self.storage.file(id, version, path).await
    }

    pub async fn diff(
        &self,
        id: uuid::Uuid,
        before: i64,
        after: i64,
        cursor: Option<String>,
    ) -> Result<WorkspaceDiff, SendableError> {
        self.storage.diff(id, before, after, cursor).await
    }

    #[cfg(test)]
    pub(super) fn objects(
        &self,
        workspace: uuid::Uuid,
    ) -> runinator_workspace::storage::cache::BufferedStore<
        super::workspace_objects::SharedObjects<T>,
    > {
        self.storage
            .as_any()
            .downcast_ref::<ObjectGraphStorageProvider<T>>()
            .expect("object-graph test helper requires the object-graph provider")
            .objects(workspace)
    }

    #[cfg(test)]
    pub(super) async fn collect_workspace(
        &self,
        workspace: uuid::Uuid,
    ) -> Result<(), SendableError> {
        self.storage
            .as_any()
            .downcast_ref::<ObjectGraphStorageProvider<T>>()
            .expect("object-graph test helper requires the object-graph provider")
            .collect_workspace(workspace)
            .await
    }
}

#[cfg(test)]
#[path = "workspace_storage_tests.rs"]
mod tests;

mod workspace_storage_context;
pub(crate) use workspace_storage_context::WorkspaceStorageContext;

mod workspace_storage_provider;
pub(crate) use workspace_storage_provider::WorkspaceStorageProvider;

mod object_graph_storage_provider;
pub(crate) use object_graph_storage_provider::ObjectGraphStorageProvider;
