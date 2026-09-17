#[allow(unused_imports)]
use super::*;

/// where a suspended cursor goes back to, snapshotted when the interrupt is raised.
///
/// restoring the whole point rather than diffing it is what makes `finish_interrupt` idempotent: a
/// duplicated drive writes the same position and frames it would have written the first time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResumePoint {
    #[serde(default)]
    pub node_id: String,
    #[serde(rename = "loops", default, skip_serializing_if = "Vec::is_empty")]
    pub loops: Vec<LoopFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub try_frame: Option<TryFrame>,
}
