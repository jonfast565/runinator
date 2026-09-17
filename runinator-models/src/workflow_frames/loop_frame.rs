#[allow(unused_imports)]
use super::*;

/// one live loop on a cursor: which loop node, and where that loop is.
///
/// the frame is authoritative. deriving the index by counting the loop node's succeeded runs made
/// an inner loop count every outer lap's runs as its own, so on the second outer pass it was
/// already past its item count and exhausted without running its body once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoopFrame {
    /// the loop node this frame belongs to; nested loops keep one frame each, keyed by this.
    #[serde(default)]
    pub node_id: String,
    /// the iteration whose body is running now, zero-based.
    #[serde(default)]
    pub index: i64,
    /// the collection snapshot resolved when this loop was entered.
    #[serde(default)]
    pub items: Vec<Value>,
    /// body outputs completed before the current lap, in item order.
    #[serde(default)]
    pub results: Vec<Value>,
    /// this loop's own node run for the current lap. anything this cursor records after it belongs
    /// to that lap's body, which is what makes `LoopOutput.last` body-scoped rather than run-wide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_node_run_id: Option<Uuid>,
}
