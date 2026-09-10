//! built-in catalog and execution share a single registration source.
use super::*;
#[test]
fn catalog_uses_registry_metadata() {
    let catalog = builtin_catalog();
    assert_eq!(catalog.len(), 3);
    for (kind, adapter) in registry() {
        assert_eq!(catalog[kind].metadata.kind, adapter.metadata().kind);
        assert_eq!(catalog[kind].origin, "builtin");
    }
}
#[tokio::test]
async fn webhook_only_poll_keeps_checkpoint() {
    let request: AdapterPollRequest = serde_json::from_value(serde_json::json!({
        "configuration": {}, "secrets": {}, "checkpoint": {"cursor":"previous"}
    }))
    .unwrap();
    let response = registry()["generic_webhook"].poll(request).await;
    assert_eq!(
        response.checkpoint,
        serde_json::json!({"cursor":"previous"})
    );
    assert!(response.events.is_empty());
    assert!(response.error.is_some());
}
