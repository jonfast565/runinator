#[allow(unused_imports)]
use super::*;

/// an outgoing edge a node kind exposes. drives the edge palette and semantic connection handles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeEdgeSlot {
    /// stable identifier for the slot: a transition key (`on_success`), or a control key
    /// (`on`, `off`, `body`, `catch`, `finally`, `branches`, `wait_for`, `cases`, `buckets`,
    /// `target`, `default`).
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub taxonomy: EdgeTaxonomy,
    /// where the target node reference is written in the node json.
    pub target: FieldLocation,
    /// whether the slot holds a list of targets (branches, wait_for, cases, buckets).
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub editable_label: bool,
    #[serde(default)]
    pub editable_condition: bool,
    #[serde(default)]
    pub orderable: bool,
}
