//! `action`: runs a task through a provider action.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorType as WorkflowType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::errors::WorkflowValidationError;
use crate::node_kinds::builders::{base, field, opt, req};
use crate::node_kinds::{ActionCatalog, GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Action;

impl NodeKindSpec for Action {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Action
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        if node.action.is_none() {
            return Err(WorkflowValidationError::MissingAction(
                node.id.as_str().to_string(),
            ));
        }
        Ok(())
    }

    fn output_type(
        &self,
        node: &WorkflowNode,
        actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| WorkflowValidationError::MissingAction(node.id.as_str().to_string()))?;
        let metadata = actions
            .get(&action.provider, &action.function)
            .ok_or_else(|| {
                WorkflowValidationError::TypeError(format!(
                    "node '{}' references unknown provider action '{}.{}'",
                    node.id, action.provider, action.function
                ))
            })?;
        Ok(Some(metadata.results_type()))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("provider", RuninatorType::String),
                    FieldLocation::action(&["provider"]),
                    Some("provider"),
                ),
                field(
                    req("function", RuninatorType::String),
                    FieldLocation::action(&["function"]),
                    Some("action_function"),
                ),
                field(
                    opt("timeout_seconds", RuninatorType::Integer),
                    FieldLocation::action(&["timeout_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "action",
                "action": { "provider": "", "function": "", "timeout_seconds": 300, "configuration": {} },
                "parameters": {},
                "retry": { "max_attempts": 1 },
                "transitions": {},
            }),
            ..base(
                self,
                "Action",
                "bolt",
                "task",
                "Runs a task through a provider action.",
            )
        }
    }
}
