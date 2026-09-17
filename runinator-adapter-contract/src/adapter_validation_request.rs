#[allow(unused_imports)]
use super::*;

/// A draft configuration submitted before an adapter definition is persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterValidationRequest {
    pub transport: AdapterTransport,
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub authentication: AdapterAuthentication,
}
