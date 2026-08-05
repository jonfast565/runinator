//! `approval`: halts until a human approves or rejects.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Approval;

impl NodeKindSpec for Approval {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Approval
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("approval_type", RuninatorType::String),
                    FieldLocation::parameters(&["approval_type"]),
                    None,
                ),
                field(
                    opt("prompt", RuninatorType::String),
                    FieldLocation::parameters(&["prompt"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "approval",
                "parameters": { "approval_type": "generic", "prompt": "Approval required" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_reject": end_ref() },
            }),
            ..base(
                self,
                "Approval",
                "approve",
                "control-flow",
                "Halts until a human approves or rejects.",
            )
        }
    }
}
