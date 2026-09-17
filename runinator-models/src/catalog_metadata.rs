//! UI-facing metadata catalogs for workflow node kinds, edges, and triggers.
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

mod field_location;
pub use field_location::FieldLocation;

mod ui_field;
pub use ui_field::UiField;

mod node_field_metadata;
pub use node_field_metadata::NodeFieldMetadata;

mod node_edge_slot;
pub use node_edge_slot::NodeEdgeSlot;

mod workflow_node_kind_metadata;
pub use workflow_node_kind_metadata::WorkflowNodeKindMetadata;

mod workflow_trigger_kind_metadata;
pub use workflow_trigger_kind_metadata::WorkflowTriggerKindMetadata;

mod enum_option_metadata;
pub use enum_option_metadata::EnumOptionMetadata;

mod enum_catalog_metadata;
pub use enum_catalog_metadata::EnumCatalogMetadata;
