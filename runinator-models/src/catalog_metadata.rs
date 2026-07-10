//! ui-facing metadata catalogs for workflow node kinds, edges, and triggers.
//!
//! the command center renders the node palette, the step editor, the read-only detail view,
//! the edge palette, and trigger forms generically from these descriptors instead of
//! hardcoding each kind. this mirrors the provider metadata pattern (`providers::ProviderMetadata`):
//! the backend owns the contract and publishes it as data, so adding a node/edge/trigger kind is a
//! backend-only change. reuse `ParameterMetadata`/`RuninatorType` for every field schema.

use serde::{Deserialize, Serialize};

use crate::providers::ParameterMetadata;
use crate::value::Value;
use crate::workflows::{WorkflowNodeKind, WorkflowTriggerKind};

/// which region of a `WorkflowNode` a field reads from and writes to. node kinds do not all
/// store their inputs under `parameters`: `wait` uses `node.wait`, `loop` uses
/// `node.max_iterations`, `action` uses `node.action`, `condition` uses `node.transitions`, etc.
/// a generic editor uses this to get/set the right json path without per-kind logic.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocationBase {
    Parameters,
    Wait,
    Condition,
    Action,
    Transitions,
    /// a direct field on the node object (e.g. `max_iterations`, `subflow_id`, `timeout_seconds`).
    TopLevel,
}

/// a json pointer relative to a `LocationBase`. `path` is a sequence of object keys.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldLocation {
    pub base: LocationBase,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
}

impl FieldLocation {
    fn new(base: LocationBase, path: &[&str]) -> Self {
        Self {
            base,
            path: path.iter().map(|segment| (*segment).to_string()).collect(),
        }
    }

    pub fn parameters(path: &[&str]) -> Self {
        Self::new(LocationBase::Parameters, path)
    }

    pub fn wait(path: &[&str]) -> Self {
        Self::new(LocationBase::Wait, path)
    }

    pub fn condition(path: &[&str]) -> Self {
        Self::new(LocationBase::Condition, path)
    }

    pub fn action(path: &[&str]) -> Self {
        Self::new(LocationBase::Action, path)
    }

    pub fn transitions(path: &[&str]) -> Self {
        Self::new(LocationBase::Transitions, path)
    }

    pub fn top_level(key: &str) -> Self {
        Self::new(LocationBase::TopLevel, &[key])
    }
}

/// a single editable field on a form. wraps the shared `ParameterMetadata` schema with an
/// optional widget hint so the frontend can pick a richer control (`cron`, `duration`,
/// `node_ref`, `json`, `expression`, ...).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UiField {
    #[serde(flatten)]
    pub param: ParameterMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget: Option<String>,
}

impl UiField {
    pub fn new(param: ParameterMetadata) -> Self {
        Self {
            param,
            widget: None,
        }
    }

    pub fn with_widget(mut self, widget: impl Into<String>) -> Self {
        self.widget = Some(widget.into());
        self
    }
}

impl From<ParameterMetadata> for UiField {
    fn from(param: ParameterMetadata) -> Self {
        Self::new(param)
    }
}

/// a form field bound to a specific location within the node json.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeFieldMetadata {
    #[serde(flatten)]
    pub field: UiField,
    pub location: FieldLocation,
}

impl NodeFieldMetadata {
    pub fn new(field: impl Into<UiField>, location: FieldLocation) -> Self {
        Self {
            field: field.into(),
            location,
        }
    }
}

/// the frontend edge classification. `direct` = a `transitions.<key>` slot; `branch` = a
/// predicate/condition branch in `transitions.branches`; `control` = a routing target stored in
/// the node's `parameters` (toggle on/off, try body/catch/finally, join wait_for, ...).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EdgeTaxonomy {
    Direct,
    Branch,
    Control,
}

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

/// full ui descriptor for one workflow node kind.
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
    /// whether this kind can host user-defined predicate edges (a `when -> target` route in
    /// `transitions.branches`, evaluated before status routing). control-flow kinds that own their
    /// routing (condition, switch, parallel, ...) and terminals do not.
    #[serde(default)]
    pub supports_predicate_edges: bool,
    #[serde(default)]
    pub fields: Vec<NodeFieldMetadata>,
    /// per-kind control-flow edges (toggle on/off, try body/catch/finally, join wait_for, ...) and
    /// the condition-branch slot. the universal direct transitions (next/on_success/on_failure/
    /// on_timeout/on_reject) are a frontend constant and are not repeated here.
    #[serde(default)]
    pub edge_slots: Vec<NodeEdgeSlot>,
    /// the default node json produced when this kind is created from the palette (minus the id).
    #[serde(default)]
    pub default_template: Value,
}

/// full ui descriptor for one workflow trigger kind. trigger config lives in the untyped
/// `configuration` blob, so fields are plain `UiField`s (no `FieldLocation`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowTriggerKindMetadata {
    pub kind: WorkflowTriggerKind,
    pub label: String,
    pub icon: String,
    pub description: String,
    #[serde(default)]
    pub fields: Vec<UiField>,
    #[serde(default)]
    pub default_configuration: Value,
}

/// one option of a small closed enum (gate kind, edge match kind, branch policy, setting kind).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumOptionMetadata {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl EnumOptionMetadata {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }
}

/// a named closed enum served for the frontend's `<select>` controls.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnumCatalogMetadata {
    /// stable name: `gate_kind`, `match_kind`, `branch_policy`, `setting_kind`.
    pub name: String,
    pub options: Vec<EnumOptionMetadata>,
}

impl EnumCatalogMetadata {
    pub fn new(name: &str, options: Vec<EnumOptionMetadata>) -> Self {
        Self {
            name: name.to_string(),
            options,
        }
    }
}
