//! cache of live provider/setting metadata used to drive completion. refreshed on a timer; a
//! failed fetch leaves the prior snapshot intact so completion degrades gracefully when the web
//! service is unreachable.

use std::sync::{Arc, RwLock};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::providers::ProviderMetadata;
use runinator_models::settings::SettingSummary;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// holds the API client used for metadata and the latest fetched snapshot.

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;

mod metadata_source;
pub use metadata_source::MetadataSource;

mod metadata_snapshot;
pub use metadata_snapshot::MetadataSnapshot;

mod metadata_cache;
pub use metadata_cache::MetadataCache;
