//! Scoped control-plane access for execution clients.

use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;

#[async_trait]
pub trait FunctionArtifactSource: Send + Sync {
    async fn download_function_artifact(&self, digest: &str) -> Result<Vec<u8>>;
}

#[async_trait]
impl<L: ServiceLocator> FunctionArtifactSource for AsyncApiClient<L> {
    async fn download_function_artifact(&self, digest: &str) -> Result<Vec<u8>> {
        AsyncApiClient::download_function_artifact(self, digest).await
    }
}
