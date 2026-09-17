#[allow(unused_imports)]
use super::*;

/// marks a cursor as a debugger-spawned "what if" branch rather than a real thread of control.
///
/// a speculative cursor walks the same graph beside the real ones, but it must not be able to change
/// what the run means: it never satisfies a join, never moves the run's status, and shadows any node
/// whose processing would escape the run unless that node is explicitly armed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeculativeFrame {
    /// the cursor this one was forked from, so a nested fork drains as a unit.
    pub forked_from_cursor: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at: DateTime<Utc>,
    /// nodes opted in to real dispatch; every other external-effect node shadows.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub armed_nodes: BTreeSet<String>,
    /// merge-patch overlaid on this cursor's resolved context, for "what if this value differed".
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub context_patch: Value,
}
