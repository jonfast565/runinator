#[allow(unused_imports)]
use super::*;

/// an aggregated `from_node -> to_node` edge across all runs of a workflow, with how often it
/// was taken and when it was last taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTransitionStat {
    pub from_node: String,
    pub to_node: String,
    pub count: i64,
    pub last_reason: Option<String>,
    pub last_at: DateTime<Utc>,
}
