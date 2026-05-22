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

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct SecretBundle {
    #[serde(default)]
    pub secrets: Vec<SecretBundleEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct SecretBundleEntry {
    pub scope: String,
    pub name: String,
    pub secret: String,
}

impl Bundle for SecretBundle {
    const RESOURCE: &'static str = "/credentials/import";
}
