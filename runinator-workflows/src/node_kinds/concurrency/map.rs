//! `map`: runs its target once for each item in a collection.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorField;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, control, end_ref, field, opt, positive_integer};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_map_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Map;

impl NodeKindSpec for Map {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Map
    }

    fn graph_role(&self) -> GraphRole {
        // the body routes back here once per item, and the simulator does not model fan-out.
        GraphRole::STEP.reentrant().not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_map_parameters(node)?;
        Ok(())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &crate::node_kinds::ActionCatalog<'_>,
    ) -> Result<Option<RuninatorType>, WorkflowValidationError> {
        Ok(Some(RuninatorType::typed_structure([
            ("item", RuninatorField::optional(RuninatorType::Any)),
            ("index", RuninatorField::optional(RuninatorType::Integer)),
            ("count", RuninatorField::optional(RuninatorType::Integer)),
            (
                "outputs",
                RuninatorField::optional(RuninatorType::array(RuninatorType::Any)),
            ),
        ])))
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(vec![TargetSlot::runnable(
            "target",
            "map target",
            parse_map_parameters(node)?.target,
        )])
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![
                field(
                    opt("items", RuninatorType::array(RuninatorType::Any)),
                    FieldLocation::parameters(&["items"]),
                    Some("expression"),
                ),
                field(
                    opt("concurrency", positive_integer()),
                    FieldLocation::parameters(&["concurrency"]),
                    None,
                ),
            ],
            edge_slots: vec![control("target", "Map target", &["target"], false)],
            default_template: json!({
                "kind": "map",
                "parameters": { "items": [], "target": end_ref(), "concurrency": 1 },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Map",
                "grid",
                "concurrency",
                "Runs its target once for each item in a collection.",
            )
        }
    }
}
