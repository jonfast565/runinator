//! Scoped control-plane access for execution clients.

use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait RunFileSource: Send + Sync {
    async fn download_workflow_file_for_run(&self, file_id: Uuid, run_id: Uuid) -> Result<Vec<u8>>;
}

#[async_trait]
impl<L: ServiceLocator> RunFileSource for AsyncApiClient<L> {
    async fn download_workflow_file_for_run(&self, file_id: Uuid, run_id: Uuid) -> Result<Vec<u8>> {
        AsyncApiClient::download_workflow_file_for_run(self, file_id, run_id).await
    }
}
