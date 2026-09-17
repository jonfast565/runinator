#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterMetadataEnvelope {
    pub abi_version: u32,
    pub metadata: AdapterKindMetadata,
}
