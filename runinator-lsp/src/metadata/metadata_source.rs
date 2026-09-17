#[allow(unused_imports)]
use super::*;

#[tower_lsp::async_trait]
pub trait MetadataSource: Send + Sync {
    async fn providers(&self) -> runinator_api::Result<Vec<ProviderMetadata>>;
    async fn settings(&self) -> runinator_api::Result<Vec<SettingSummary>>;
}

#[tower_lsp::async_trait]
impl MetadataSource for AsyncApiClient<StaticLocator> {
    async fn providers(&self) -> runinator_api::Result<Vec<ProviderMetadata>> {
        self.fetch_providers().await
    }
    async fn settings(&self) -> runinator_api::Result<Vec<SettingSummary>> {
        self.list_settings().await
    }
}
