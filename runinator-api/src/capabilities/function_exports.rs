//! Scoped control-plane access for execution clients.

use crate::{AsyncApiClient, Result, ServiceLocator};
use async_trait::async_trait;
use runinator_models::functions::FunctionInvocationTarget;
use uuid::Uuid;

#[async_trait]
pub trait FunctionExportResolver: Send + Sync {
    async fn resolve_function_export(&self, export_id: Uuid) -> Result<FunctionInvocationTarget>;
}

#[async_trait]
impl<L: ServiceLocator> FunctionExportResolver for AsyncApiClient<L> {
    async fn resolve_function_export(&self, export_id: Uuid) -> Result<FunctionInvocationTarget> {
        AsyncApiClient::resolve_function_export(self, export_id).await
    }
}
