#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowReentryFrame {
    pub reentry_key: String,
    #[serde(default)]
    pub visits: u64,
    pub max_visits: u64,
}
