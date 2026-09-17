#[allow(unused_imports)]
use super::*;

/// a point-in-time copy of the metadata used by `complete_source`.
#[derive(Clone, Default)]
pub struct MetadataSnapshot {
    pub providers: Vec<ProviderMetadata>,
    pub settings: Vec<SettingSummary>,
}
