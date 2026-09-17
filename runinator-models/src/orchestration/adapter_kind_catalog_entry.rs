#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterKindCatalogEntry {
    pub metadata: AdapterKindMetadata,
    pub origin: String,
    pub healthy: bool,
    #[serde(default)]
    pub error: Option<String>,
}
