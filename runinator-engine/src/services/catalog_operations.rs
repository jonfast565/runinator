//! application service for durable authoring catalog items.

use std::sync::Arc;

use runinator_models::{errors::SendableError, value::Value};
use runinator_store::roles::DefinitionStore;

use crate::repository;

/// Provides catalog persistence operations to transport adapters.
#[derive(Clone)]
pub struct CatalogOperations<T> {
    store: Arc<T>,
}

impl<T> CatalogOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: DefinitionStore> CatalogOperations<T> {
    pub async fn list(&self, item_type: Option<String>) -> Result<Vec<Value>, SendableError> {
        repository::fetch_catalog_items(self.store.as_ref(), item_type).await
    }

    pub async fn fetch(&self, uri: String) -> Result<Option<Value>, SendableError> {
        repository::fetch_catalog_item(self.store.as_ref(), uri).await
    }

    pub async fn upsert(&self, item: Value) -> Result<Value, SendableError> {
        repository::upsert_catalog_item(self.store.as_ref(), item).await
    }
}

/// The one canonical row shape for provider metadata, shared by all catalog writers.
pub fn provider_catalog_item(provider: &runinator_models::providers::ProviderMetadata) -> Value {
    repository::provider_catalog_item(provider)
}
