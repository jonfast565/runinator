#[allow(unused_imports)]
use super::*;

/// one edge walked by a workflow run, derived from its immutable VM journal.
/// `from_node` is `None` for the run's first node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTransition {
    pub from_node: Option<String>,
    pub to_node: String,
    pub reason: Option<String>,
    /// Journal record identity retained under the historical wire name for client compatibility.
    pub node_run_id: Uuid,
    pub at: DateTime<Utc>,
}
