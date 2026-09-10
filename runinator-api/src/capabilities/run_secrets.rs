//! Scoped control-plane access for execution clients.

use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait RunSecretReader: Send + Sync {
    async fn fetch_credential_by_id_for_run(&self, id: Uuid, run_id: Uuid) -> Result<String>;
}

#[async_trait]
impl<L: ServiceLocator> RunSecretReader for AsyncApiClient<L> {
    async fn fetch_credential_by_id_for_run(&self, id: Uuid, run_id: Uuid) -> Result<String> {
        AsyncApiClient::fetch_credential_by_id_for_run(self, id, run_id).await
    }
}
