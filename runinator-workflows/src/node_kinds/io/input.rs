//! `input`: waits for a user-supplied value from the UI.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorType as WorkflowType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, field, opt};
use crate::node_kinds::{ActionCatalog, GraphRole, NodeKindSpec};
use crate::parameters::parse_input_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Input;

impl NodeKindSpec for Input {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Input
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        let _ = parse_input_parameters(node);
        Ok(())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        Ok(Some(WorkflowType::Any))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                opt("prompt", RuninatorType::String),
                FieldLocation::parameters(&["prompt"]),
                None,
            )],
            default_template: json!({
                "kind": "input", "parameters": { "prompt": "Provide input" },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Input",
                "message",
                "io",
                "Waits for a user-supplied value from the UI.",
            )
        }
    }
}
