//! `throttle`: enforces a cross-run rate limit; parks until a token is available.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Throttle;

impl NodeKindSpec for Throttle {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Throttle
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
                    opt("max_per_window", RuninatorType::Integer),
                    FieldLocation::parameters(&["max_per_window"]),
                    None,
                ),
                field(
                    opt("window_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["window_seconds"]),
                    None,
                ),
                field(
                    opt("poll_interval_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "throttle",
                "parameters": { "name": "my-throttle", "max_per_window": 10, "window_seconds": 60 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Throttle",
                "hourglass",
                "sync",
                "Enforces a cross-run rate limit; parks until a token is available.",
            )
        }
    }
}
