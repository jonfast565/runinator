//! Scoped control-plane access for execution clients.

use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;
use runinator_models::execution_profiles::ExecutionProfile;
use uuid::Uuid;

#[async_trait]
pub trait ExecutionProfileSource: Send + Sync {
    async fn resolve_execution_profile_for_run(
        &self,
        name: &str,
        run_id: Uuid,
    ) -> Result<ExecutionProfile>;
    async fn fetch_execution_profile_for_run(
        &self,
        id: Uuid,
        run_id: Uuid,
    ) -> Result<ExecutionProfile>;
    async fn fetch_execution_profile_for_adapter_dispatch(
        &self,
        id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<ExecutionProfile>;
    async fn download_execution_profile_for_run(
        &self,
        id: Uuid,
        revision: i64,
        run_id: Uuid,
    ) -> Result<Vec<u8>>;
    async fn download_execution_profile_for_adapter_dispatch(
        &self,
        id: Uuid,
        revision: i64,
        dispatch_id: Uuid,
    ) -> Result<Vec<u8>>;
}

#[async_trait]
impl<L: ServiceLocator> ExecutionProfileSource for AsyncApiClient<L> {
    async fn resolve_execution_profile_for_run(
        &self,
        name: &str,
        run_id: Uuid,
    ) -> Result<ExecutionProfile> {
        AsyncApiClient::resolve_execution_profile_for_run(self, name, run_id).await
    }
    async fn fetch_execution_profile_for_run(
        &self,
        id: Uuid,
        run_id: Uuid,
    ) -> Result<ExecutionProfile> {
        AsyncApiClient::fetch_execution_profile_for_run(self, id, run_id).await
    }
    async fn fetch_execution_profile_for_adapter_dispatch(
        &self,
        id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<ExecutionProfile> {
        AsyncApiClient::fetch_execution_profile_for_adapter_dispatch(self, id, dispatch_id).await
    }
    async fn download_execution_profile_for_run(
        &self,
        id: Uuid,
        revision: i64,
        run_id: Uuid,
    ) -> Result<Vec<u8>> {
        AsyncApiClient::download_execution_profile_for_run(self, id, revision, run_id).await
    }
    async fn download_execution_profile_for_adapter_dispatch(
        &self,
        id: Uuid,
        revision: i64,
        dispatch_id: Uuid,
    ) -> Result<Vec<u8>> {
        AsyncApiClient::download_execution_profile_for_adapter_dispatch(
            self,
            id,
            revision,
            dispatch_id,
        )
        .await
    }
}
