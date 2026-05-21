use serde::{Serialize, de::DeserializeOwned};

use crate::providers::ProviderMetadata;

/// Marker trait for typed import bundles posted to the web service.
///
/// Implementations advertise their HTTP resource path so the API client and
/// importer can be generic over bundle kind.
pub trait Bundle: Serialize + DeserializeOwned + Send + Sync + 'static {
    const RESOURCE: &'static str;
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct ProviderBundle {
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
}

impl Bundle for ProviderBundle {
    const RESOURCE: &'static str = "/providers/import";
}
