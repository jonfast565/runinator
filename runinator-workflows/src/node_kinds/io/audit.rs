//! `audit`: appends a tamper-evident audit record to the workflow log.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Audit;

impl NodeKindSpec for Audit {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Audit
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("action", RuninatorType::Any),
                    FieldLocation::parameters(&["action"]),
                    Some("expression"),
                ),
                field(
                    opt("actor", RuninatorType::Any),
                    FieldLocation::parameters(&["actor"]),
                    Some("expression"),
                ),
                field(
                    opt("target", RuninatorType::Any),
                    FieldLocation::parameters(&["target"]),
                    Some("expression"),
                ),
                field(
                    opt("reason", RuninatorType::Any),
                    FieldLocation::parameters(&["reason"]),
                    Some("expression"),
                ),
            ],
            default_template: json!({
                "kind": "audit", "parameters": { "action": "" },
                "retry": { "max_attempts": 1 }, "transitions": { "next": end_ref() },
            }),
            ..base(
                self,
                "Audit",
                "file",
                "io",
                "Appends a tamper-evident audit record to the workflow log.",
            )
        }
    }
}
