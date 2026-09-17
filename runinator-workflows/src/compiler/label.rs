#[allow(unused_imports)]
use super::*;

/// A symbolic basic-block address.  Keeping these until the final layout pass means graph
/// traversal never needs to guess an instruction offset while it is lowering a node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Label(pub(super) String);

impl Label {
    pub(super) fn node(node_id: &str) -> Self {
        Self(node_id.to_owned())
    }

    /// A synthetic block owned by `node_id`. The `#` prefix cannot collide with a graph node id,
    /// which validation restricts to identifier characters.
    pub(super) fn synthetic(node_id: &str, suffix: &str) -> Self {
        Self(format!("{node_id}#{suffix}"))
    }

    /// The edge slot a synthetic block stands for (`on_failure`, `on_timeout`, ...), or `None` for
    /// a node's own block. This is what tells an operator watching a cursor which edge it took.
    pub(super) fn edge_slot(&self) -> Option<&str> {
        self.0.split_once('#').map(|(_, slot)| slot)
    }
}
