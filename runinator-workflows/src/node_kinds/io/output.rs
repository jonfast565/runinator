//! `output`: publishes output without interrupting the flow.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorType as WorkflowType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, field, opt};
use crate::node_kinds::{ActionCatalog, GraphRole, NodeKindSpec};
use crate::parameters::parse_output_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Output;

impl NodeKindSpec for Output {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Output
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_output_parameters(node)?;
        Ok(())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        Ok(Some(WorkflowType::structure([
            ("event_type", WorkflowType::String),
            ("data", WorkflowType::Any),
            ("artifacts", WorkflowType::Any),
        ])))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("event_type", RuninatorType::String),
                    FieldLocation::parameters(&["event_type"]),
                    None,
                ),
                field(
                    opt("data", RuninatorType::Any),
                    FieldLocation::parameters(&["data"]),
                    Some("json"),
                ),
            ],
            default_template: json!({
                "kind": "output",
                "parameters": { "event_type": "workflow.output", "data": {} },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Output",
                "output",
                "io",
                "Publishes output without interrupting the flow.",
            )
        }
    }
}
