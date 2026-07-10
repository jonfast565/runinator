use runinator_models::providers::ProviderMetadata;
use runinator_models::value::Value;

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
