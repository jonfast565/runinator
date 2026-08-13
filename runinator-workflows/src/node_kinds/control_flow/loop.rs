//! `loop`: repeats its body once for each item in a collection.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Loop;

impl NodeKindSpec for Loop {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Loop
    }

    fn graph_role(&self) -> GraphRole {
        // the body edge returns here every iteration, and the simulator does not model iteration.
        GraphRole::STEP.reentrant().not_simulatable()
    }

    // no target slots: a loop's body is `transitions.next` and its exit is `transitions.on_success`.
    // the reducer never reads a `parameters.target`, and the wdl lowering never emits one.

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
                    opt("max_iterations", RuninatorType::Integer),
                    FieldLocation::top_level("max_iterations"),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "loop",
                "parameters": { "items": [] },
                "max_iterations": 10,
                "retry": { "max_attempts": 1 }, "transitions": {},
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
