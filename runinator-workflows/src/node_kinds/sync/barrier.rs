//! `barrier`: parks until N runs reach this named barrier; the last arrival releases all waiters.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{
    base, end_ref, field, opt, positive_duration, positive_integer, req,
};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Barrier;

impl NodeKindSpec for Barrier {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Barrier
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("count", positive_integer()),
                    FieldLocation::parameters(&["count"]),
                    None,
                ),
                field(
                    opt("poll_interval_seconds", positive_duration()),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "barrier", "parameters": { "name": "my-barrier", "count": 2 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Barrier",
                "join",
                "sync",
                "Parks until N runs reach this named barrier; the last arrival releases all waiters.",
            )
        }
    }
}
