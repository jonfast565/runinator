#[allow(unused_imports)]
use super::*;

/// operating limits used by embedded and standalone durable engine runtimes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BackgroundEngineSettings {
    pub max_concurrent_ingress: u64,
}

impl Default for BackgroundEngineSettings {
    fn default() -> Self {
        Self {
            max_concurrent_ingress: 16,
        }
    }
}
