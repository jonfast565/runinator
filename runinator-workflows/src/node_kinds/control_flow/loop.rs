//! `loop`: repeats its body once for each item in a collection.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorField;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, direct, field, opt, positive_integer};
use crate::node_kinds::{GraphRole, NodeKindSpec};
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Loop;

impl NodeKindSpec for Loop {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Loop
    }

    fn graph_role(&self) -> GraphRole {
        // the body edge returns here every iteration.
        GraphRole::STEP.reentrant()
    }

    // no target slots: a loop's body is `transitions.next` and its exit is `transitions.on_success`.
    // the reducer never reads a `parameters.target`, and the rexrap lowering never emits one.

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        // presence only. an authored `items` is normally a `$ref` expression, so its arrayness is
        // not knowable here — `typing.rs` checks that against the inferred type, and the reducer
        // checks the resolved value. this is the graph-independent half the trait asks for.
        if node.parameters.get("items").is_none() {
            return Err(WorkflowValidationError::InvalidNodeParameters {
                node: node.id.as_str().to_string(),
                message: "loop.items is required".into(),
            });
        }
        Ok(())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &crate::node_kinds::ActionCatalog<'_>,
    ) -> Result<Option<RuninatorType>, WorkflowValidationError> {
        Ok(Some(RuninatorType::typed_structure([
            ("item", RuninatorField::optional(RuninatorType::Any)),
            ("index", RuninatorField::required(RuninatorType::Integer)),
            ("has_next", RuninatorField::required(RuninatorType::Boolean)),
            ("count", RuninatorField::required(RuninatorType::Integer)),
            ("last", RuninatorField::optional(RuninatorType::Any)),
            (
                "results",
                RuninatorField::required(RuninatorType::array(RuninatorType::Any)),
            ),
        ])))
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
                    opt("max_iterations", positive_integer()),
                    FieldLocation::top_level("max_iterations"),
                    None,
                ),
            ],
            edge_slots: vec![
                direct("next", "Loop body"),
                direct("on_success", "Loop exit"),
            ],
            default_template: json!({
                "kind": "loop",
                "parameters": { "items": [] },
                "max_iterations": 10,
                "retry": { "max_attempts": 1 },
                "transitions": { "next": null, "on_success": null },
            }),
            ..base(
                self,
                "Loop",
                "loop",
                "control-flow",
                "Repeats its body once for each item in a collection.",
            )
        }
    }
}
