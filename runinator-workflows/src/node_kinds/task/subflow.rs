//! `subflow`: invokes another workflow as a nested step.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorType as WorkflowType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::errors::WorkflowValidationError;
use crate::node_kinds::builders::{base, field, req};
use crate::node_kinds::{ActionCatalog, GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Subflow;

impl NodeKindSpec for Subflow {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Subflow
    }

    fn graph_role(&self) -> GraphRole {
        // a subflow spawns a child run the walk cannot model.
        GraphRole::STEP.not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        if node.subflow_id.is_none()
            && node
                .subflow
                .workflow_name
                .as_ref()
                .is_none_or(|name| name.trim().is_empty())
        {
            return Err(WorkflowValidationError::MissingSubflowTarget(
                node.id.as_str().to_string(),
            ));
        }
        Ok(())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        Ok(Some(WorkflowType::structure([
            ("subflow_run_id", WorkflowType::String),
            ("subflow_workflow_id", WorkflowType::String),
            ("run_name", WorkflowType::String),
            ("reused", WorkflowType::Boolean),
            ("status", WorkflowType::String),
            ("state", WorkflowType::Any),
            ("parameters", WorkflowType::Any),
        ])))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                req("subflow_id", RuninatorType::String),
                FieldLocation::top_level("subflow_id"),
                Some("subflow"),
            )],
            default_template: json!({
                "kind": "subflow", "subflow_id": null, "parameters": {},
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Subflow",
                "workflow",
                "task",
                "Invokes another workflow as a nested step.",
            )
        }
    }
}
