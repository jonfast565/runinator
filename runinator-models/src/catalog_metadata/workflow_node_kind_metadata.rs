#[allow(unused_imports)]
use super::*;

/// full UI descriptor for one workflow node kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowNodeKindMetadata {
    pub kind: WorkflowNodeKind,
    pub label: String,
    pub icon: String,
    pub description: String,
    /// grouping used by the palette: `task`, `control-flow`, `concurrency`, `io`, `sync`, `terminal`.
    pub category: String,
    /// start/end/fail: cannot be deleted and their kind cannot change.
    #[serde(default)]
    pub protected: bool,
    /// a terminal node (end/fail): has no outgoing edges.
    #[serde(default)]
    pub terminal: bool,
    /// whether this kind appears in the "add node" palette (start/end/fail do not).
    #[serde(default)]
    pub addable: bool,
    /// may appear inside an interrupt handler region. an opt-in allowlist: a kind that could park
    /// or fan out inside a handler is not on it. the header editor reads this to validate a region
    /// and to pick what it scaffolds, rather than keeping a second copy of the list.
    #[serde(default)]
    pub handler_safe: bool,
    /// may be entered as a branch, body, or handler-region target — true for everything but
    /// `start`/`end`/`fail`.
    #[serde(default)]
    pub runnable_entry: bool,
    /// an entry point the runtime places a cursor on directly: `start` and `interrupt`. no edge may
    /// target one, which is the rule the graph editor enforces when it offers a connection.
    #[serde(default)]
    pub entry_point: bool,
    /// whether this kind can host user-defined predicate edges (a `when -> target` route in
    /// `transitions.branches`, evaluated before status routing). control-flow kinds that own their
    /// routing (condition, switch, parallel, ...) and terminals do not.
    #[serde(default)]
    pub supports_predicate_edges: bool,
    #[serde(default)]
    pub fields: Vec<NodeFieldMetadata>,
    /// per-kind control-flow edges and semantic overrides for direct transitions. universal direct
    /// transitions remain available in the frontend even when a kind does not rename them here.
    #[serde(default)]
    pub edge_slots: Vec<NodeEdgeSlot>,
    /// output shape known from the kind's default node, for generic authoring surfaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_type: Option<crate::types::RuninatorType>,
    /// the default node json produced when this kind is created from the palette (minus the id).
    #[serde(default)]
    pub default_template: Value,
}
