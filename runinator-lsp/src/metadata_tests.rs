//! partial refreshes retain the last successful metadata independently.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
struct Source {
    fail: AtomicBool,
}
#[tower_lsp::async_trait]
impl MetadataSource for Source {
    async fn providers(&self) -> runinator_api::Result<Vec<ProviderMetadata>> {
        if self.fail.load(Ordering::SeqCst) {
            return Ok(vec![ProviderMetadata {
                name: "fresh".into(),
                actions: vec![],
                metadata: Default::default(),
            }]);
        }
        Err(runinator_api::ApiError::UnexpectedResponse(
            "offline".into(),
        ))
    }
    async fn settings(&self) -> runinator_api::Result<Vec<SettingSummary>> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(runinator_api::ApiError::UnexpectedResponse(
                "offline".into(),
            ));
        }
        Ok(vec![serde_json::from_value(serde_json::json!({ "id": "00000000-0000-0000-0000-000000000001", "scope": "global", "name": "new" })).unwrap()])
    }
}
#[tokio::test]
async fn successful_half_updates_and_failure_keeps_stale_snapshot() {
    let source = Arc::new(Source {
        fail: AtomicBool::new(false),
    });
    let cache = MetadataCache::with_source(source.clone());
    cache.snapshot.write().unwrap().providers = vec![ProviderMetadata {
        name: "cached".into(),
        actions: vec![],
        metadata: Default::default(),
    }];
    cache.refresh().await;
    assert_eq!(cache.snapshot().providers[0].name, "cached");
    assert_eq!(cache.snapshot().settings[0].name, "new");
    source.fail.store(true, Ordering::SeqCst);
    cache.refresh().await;
    assert_eq!(cache.snapshot().providers[0].name, "fresh");
    assert_eq!(cache.snapshot().settings[0].name, "new");
}
