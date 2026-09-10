//! Scoped control-plane access for execution clients.

use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait WorkspaceObjectTransport: Send + Sync {
    async fn workspace_object(
        &self,
        checkout: Uuid,
        replica: Uuid,
        id: &str,
    ) -> Result<Option<Vec<u8>>>;
    async fn upload_workspace_pack(
        &self,
        checkout: Uuid,
        replica: Uuid,
        bytes: Vec<u8>,
    ) -> Result<()>;
}

#[async_trait]
impl<L: ServiceLocator> WorkspaceObjectTransport for AsyncApiClient<L> {
    async fn workspace_object(
        &self,
        checkout: Uuid,
        replica: Uuid,
        id: &str,
    ) -> Result<Option<Vec<u8>>> {
        AsyncApiClient::workspace_object(self, checkout, replica, id).await
    }
    async fn upload_workspace_pack(
        &self,
        checkout: Uuid,
        replica: Uuid,
        bytes: Vec<u8>,
    ) -> Result<()> {
        AsyncApiClient::upload_workspace_pack(self, checkout, replica, bytes).await
    }
}
