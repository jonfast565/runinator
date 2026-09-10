//! cache of live provider/setting metadata used to drive completion. refreshed on a timer; a
//! failed fetch leaves the prior snapshot intact so completion degrades gracefully when the web
//! service is unreachable.

use std::sync::{Arc, RwLock};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::providers::ProviderMetadata;
use runinator_models::settings::SettingSummary;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

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

/// a point-in-time copy of the metadata used by `complete_source`.
#[derive(Clone, Default)]
pub struct MetadataSnapshot {
    pub providers: Vec<ProviderMetadata>,
    pub settings: Vec<SettingSummary>,
}

/// holds the API client used for metadata and the latest fetched snapshot.
pub struct MetadataCache {
    source: Arc<dyn MetadataSource>,
    snapshot: RwLock<MetadataSnapshot>,
}

impl MetadataCache {
    pub fn new(base_url: String) -> Result<Self, BoxError> {
        let client = AsyncApiClient::new(StaticLocator::new(base_url))?;
        Ok(Self::with_source(Arc::new(client)))
    }

    pub fn with_source(source: Arc<dyn MetadataSource>) -> Self {
        Self {
            source,
            snapshot: RwLock::new(MetadataSnapshot::default()),
        }
    }

    /// fetch providers and settings, swapping each into the cache only on success.
    pub async fn refresh(&self) {
        let providers = self.source.providers().await;
        let settings = self.source.settings().await;
        if let Ok(mut snapshot) = self.snapshot.write() {
            if let Ok(providers) = providers {
                snapshot.providers = providers;
            }
            if let Ok(settings) = settings {
                snapshot.settings = settings;
            }
        }
    }

    /// clone the current snapshot for use in a completion request.
    pub fn snapshot(&self) -> MetadataSnapshot {
        self.snapshot
            .read()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
