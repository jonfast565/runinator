//! `config`: sets configuration values for downstream nodes.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorType as WorkflowType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, field, opt};
use crate::node_kinds::{ActionCatalog, GraphRole, NodeKindSpec};
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Config;

impl NodeKindSpec for Config {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Config
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        Ok(Some(WorkflowType::structure([
            ("name", WorkflowType::String),
            ("metadata", WorkflowType::Any),
        ])))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("name", RuninatorType::Any),
                    FieldLocation::parameters(&["name"]),
                    Some("json"),
                ),
                field(
                    opt("metadata", RuninatorType::Any),
                    FieldLocation::parameters(&["metadata"]),
                    Some("json"),
                ),
            ],
            default_template: json!({
                "kind": "config", "parameters": { "name": "", "metadata": {} },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Config",
                "gear",
                "io",
                "Sets configuration values for downstream nodes.",
            )
        }
    }
}
