//! `assert`: evaluates named boolean assertions; fails with a structured violation list.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Assert;

impl NodeKindSpec for Assert {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Assert
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.handler_safe()
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                opt(
                    "assertions",
                    RuninatorType::Array(Box::new(RuninatorType::Any)),
                ),
                FieldLocation::parameters(&["assertions"]),
                Some("assertions"),
            )],
            default_template: json!({
                "kind": "assert", "parameters": { "assertions": [] },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Assert",
                "check",
                "control-flow",
                "Evaluates named boolean assertions; fails with a structured violation list.",
            )
        }
    }
}
