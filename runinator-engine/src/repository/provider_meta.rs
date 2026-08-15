use runinator_models::providers::ProviderMetadata;
use runinator_models::value::Value;

/// the catalog row one provider's metadata is stored as.
///
/// lives here rather than in a handler because publishing a packaged function writes the same shape
/// from the engine: a `functions.<pkg>` provider is an ordinary catalog row, so both writers must
/// build it identically or the readers would see two shapes for one thing.
pub fn provider_catalog_item(provider: &ProviderMetadata) -> Value {
    runinator_models::json!({
        "uri": provider_catalog_uri(&provider.name),
        "item_type": "provider_metadata",
        "name": provider.name,
        "version": "1",
        "document": provider,
        "metadata": {}
    })
}

/// the catalog uri a provider name is stored under.
pub fn provider_catalog_uri(name: &str) -> String {
    format!("runinator://providers/{name}")
}

/// deserialize provider metadata catalog items, sorted by provider name.
pub fn provider_metadata_from_items(
    items: Vec<Value>,
) -> Result<Vec<ProviderMetadata>, serde_json::Error> {
    let mut providers = items
        .into_iter()
        .map(provider_metadata_from_item)
        .collect::<Result<Vec<_>, _>>()?;
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(providers)
}

/// deserialize a single provider metadata catalog item, unwrapping the stored `document` envelope.
pub fn provider_metadata_from_item(item: Value) -> Result<ProviderMetadata, serde_json::Error> {
    let document = item.get("document").cloned().unwrap_or(item);
    serde_json::from_value(document.into())
}
