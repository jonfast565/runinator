#[allow(unused_imports)]
use super::*;

/// one in-flight map item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapChild {
    pub index: i64,
    pub child_run_id: Uuid,
}
