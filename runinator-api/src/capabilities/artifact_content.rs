//! Scoped control-plane access for execution clients.

use crate::ArtifactContentResponse;
use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait ArtifactContentUploader: Send + Sync {
    async fn upload_artifact_content(
        &self,
        run_id: Uuid,
        name: &str,
        mime_type: &str,
        bytes: Vec<u8>,
    ) -> Result<ArtifactContentResponse>;
}

#[async_trait]
impl<L: ServiceLocator> ArtifactContentUploader for AsyncApiClient<L> {
    async fn upload_artifact_content(
        &self,
        run_id: Uuid,
        name: &str,
        mime_type: &str,
        bytes: Vec<u8>,
    ) -> Result<ArtifactContentResponse> {
        AsyncApiClient::upload_artifact_content(self, run_id, name, mime_type, bytes).await
    }
}
