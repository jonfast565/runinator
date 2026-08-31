//! `mutex`: acquires a named fifo mutex, held by one cursor until run end or a matching release.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt, positive_duration, req};
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
                    opt("poll_interval_seconds", positive_duration()),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
                field(
                    opt("release", RuninatorType::Boolean),
                    FieldLocation::parameters(&["release"]),
                    None,
                ),
                field(
                    opt("hold_timeout_seconds", positive_duration()),
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
                "Acquires a cursor-scoped FIFO mutex; an overdue active holder remains exclusive.",
            )
        }
    }
}
