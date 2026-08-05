//! `mutex`: acquires a named distributed mutex, held until the run ends or a matching release node.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Mutex;

impl NodeKindSpec for Mutex {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Mutex
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
                    opt("poll_interval_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
                field(
                    opt("release", RuninatorType::Boolean),
                    FieldLocation::parameters(&["release"]),
                    None,
                ),
                field(
                    opt("hold_timeout_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["hold_timeout_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "mutex", "parameters": { "name": "my-mutex" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Mutex",
                "lock",
                "sync",
                "Acquires a named distributed mutex, held until the run ends or a matching release node.",
            )
        }
    }
}
