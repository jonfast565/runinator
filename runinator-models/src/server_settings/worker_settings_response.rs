#[allow(unused_imports)]
use super::*;

/// worker-facing policy response. until the unified policy has been saved, workers preserve their
/// explicit CLI configuration instead of replacing it with compiled server defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSettingsResponse {
    pub configured: bool,
    pub values: WorkerSettings,
}
