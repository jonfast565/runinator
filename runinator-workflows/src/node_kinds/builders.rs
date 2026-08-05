//! small field/edge builders shared by the node-kind specs.

use runinator_models::catalog_metadata::{
    EdgeTaxonomy, FieldLocation, NodeEdgeSlot, NodeFieldMetadata, UiField, WorkflowNodeKindMetadata,
};
use runinator_models::json;
use runinator_models::providers::{ParameterMetadata, RuninatorType};
use runinator_models::value::Value;

use super::NodeKindSpec;

pub(crate) fn req(name: &str, ty: RuninatorType) -> ParameterMetadata {
    ParameterMetadata::required(name, ty)
}

pub(crate) fn opt(name: &str, ty: RuninatorType) -> ParameterMetadata {
    ParameterMetadata::optional(name, ty)
}

pub(crate) fn enum_ty(values: &[&str]) -> RuninatorType {
    RuninatorType::Enum(
        values
            .iter()
            .map(|v| Value::String((*v).to_string()))
            .collect(),
    )
}

/// a field bound to a node-json location, with an optional widget hint.
pub(crate) fn field(
    param: ParameterMetadata,
    location: FieldLocation,
    widget: Option<&str>,
) -> NodeFieldMetadata {
    let ui = match widget {
        Some(widget) => UiField::new(param).with_widget(widget),
        None => UiField::new(param),
    };
    NodeFieldMetadata::new(ui, location)
}

/// a per-kind control-flow edge whose target is stored in the node's parameters.
///
/// `key` must match the key of the [`super::TargetSlot`] the same spec yields for that edge; a
/// conformance test fails if the two ever drift.
pub(crate) fn control(key: &str, label: &str, path: &[&str], multiple: bool) -> NodeEdgeSlot {
    NodeEdgeSlot {
        key: key.to_string(),
        label: label.to_string(),
        description: None,
        taxonomy: EdgeTaxonomy::Control,
        target: FieldLocation::parameters(path),
        multiple,
        editable_label: false,
        editable_condition: false,
        orderable: multiple,
    }
}

/// the condition-branch slot: a list of `when -> target` routes in `transitions.branches`.
pub(crate) fn condition_branches() -> NodeEdgeSlot {
    NodeEdgeSlot {
        key: "branches".to_string(),
        label: "Condition branch".to_string(),
        description: Some("A conditional route taken when its predicate matches.".to_string()),
        taxonomy: EdgeTaxonomy::Branch,
        target: FieldLocation::transitions(&["branches"]),
        multiple: true,
        editable_label: true,
        editable_condition: true,
        orderable: true,
    }
}

/// base descriptor for a node kind: predicate edges on, not protected, addable.
///
/// `kind` and `terminal` are read off the spec rather than passed in, so the palette entry and the
/// graph walkers cannot disagree about which nodes settle a run.
pub(crate) fn base(
    spec: &dyn NodeKindSpec,
    label: &str,
    icon: &str,
    category: &str,
    description: &str,
) -> WorkflowNodeKindMetadata {
    WorkflowNodeKindMetadata {
        kind: spec.kind(),
        label: label.to_string(),
        icon: icon.to_string(),
        description: description.to_string(),
        category: category.to_string(),
        protected: false,
        terminal: spec.graph_role().terminal,
        addable: true,
        supports_predicate_edges: true,
        fields: Vec::new(),
        edge_slots: Vec::new(),
        default_template: Value::Null,
    }
}

pub(crate) fn end_ref() -> Value {
    json!({ "$node": "end" })
}
