//! `checkpoint`: snapshots run state at a named point; enables rollback via the control-plane API.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Checkpoint;

impl NodeKindSpec for Checkpoint {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Checkpoint
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                req("name", RuninatorType::String),
                FieldLocation::parameters(&["name"]),
                None,
            )],
            default_template: json!({
                "kind": "checkpoint", "parameters": { "name": "checkpoint" },
                "retry": { "max_attempts": 1 }, "transitions": { "next": end_ref() },
            }),
            ..base(
                self,
                "Checkpoint",
                "save",
                "control-flow",
                "Snapshots run state at a named point; enables rollback via the control-plane API.",
            )
        }
    }
}
