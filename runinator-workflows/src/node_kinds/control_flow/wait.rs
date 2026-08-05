//! `wait`: pauses the run for a fixed delay or until a time.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Wait;

impl NodeKindSpec for Wait {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Wait
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("seconds", RuninatorType::Duration),
                    FieldLocation::wait(&["seconds"]),
                    Some("duration"),
                ),
                field(
                    opt("initial_status", RuninatorType::String),
                    FieldLocation::wait(&["initial_status"]),
                    None,
                ),
                field(
                    opt("until_status", RuninatorType::String),
                    FieldLocation::wait(&["until_status"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "wait", "wait": { "seconds": 60 },
                "parameters": {}, "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Wait",
                "clock",
                "control-flow",
                "Pauses the run for a fixed delay or until a time.",
            )
        }
    }
}
