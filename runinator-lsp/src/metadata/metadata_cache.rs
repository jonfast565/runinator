#[allow(unused_imports)]
use super::*;

pub struct MetadataCache {
    pub(super) source: Arc<dyn MetadataSource>,
    pub(super) snapshot: RwLock<MetadataSnapshot>,
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
        let Ok(mut snapshot) = self.snapshot.write() else {
            return;
        };

        if let Ok(providers) = providers {
            snapshot.providers = providers;
        }
        if let Ok(settings) = settings {
            snapshot.settings = settings;
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
