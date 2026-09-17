#[allow(unused_imports)]
use super::*;

/// operating limits carried to broker-only wakers on wake deliveries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WakerSettings {
    pub max_concurrent_wakes: u64,
    pub max_wake_sleep_seconds: u64,
}

impl Default for WakerSettings {
    fn default() -> Self {
        Self {
            max_concurrent_wakes: 32,
            max_wake_sleep_seconds: 20,
        }
    }
}
