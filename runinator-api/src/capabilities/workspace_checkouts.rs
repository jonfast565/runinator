//! Scoped control-plane access for execution clients.

use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;
use std::time::Duration;
use uuid::Uuid;

#[async_trait]
pub trait WorkspaceCheckoutClient: Send + Sync {
    async fn download_workspace_checkout(
        &self,
        checkout: Uuid,
        replica: Uuid,
        timeout: Duration,
    ) -> Result<Vec<u8>>;
    async fn seal_workspace(
        &self,
        checkout: Uuid,
        replica: Uuid,
        revision_id: String,
        timeout: Duration,
    ) -> Result<runinator_models::workspaces::WorkspaceReceipt>;
}

#[async_trait]
impl<L: ServiceLocator> WorkspaceCheckoutClient for AsyncApiClient<L> {
    async fn download_workspace_checkout(
        &self,
        checkout: Uuid,
        replica: Uuid,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        AsyncApiClient::download_workspace_checkout(self, checkout, replica, timeout).await
    }
    async fn seal_workspace(
        &self,
        checkout: Uuid,
        replica: Uuid,
        revision_id: String,
        timeout: Duration,
    ) -> Result<runinator_models::workspaces::WorkspaceReceipt> {
        AsyncApiClient::seal_workspace(self, checkout, replica, revision_id, timeout).await
    }
}
