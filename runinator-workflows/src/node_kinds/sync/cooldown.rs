//! `cooldown`: short-circuits the run to success when a prior pass ran within the window; at most one pass proceeds per window.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Cooldown;

impl NodeKindSpec for Cooldown {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Cooldown
    }

    fn graph_role(&self) -> GraphRole {
        // short-circuits the run rather than recording a value downstream nodes read.
        GraphRole::STEP.without_output()
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
                    opt("window_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["window_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "cooldown",
                "parameters": { "name": "my-cooldown", "window_seconds": 900 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref() },
            }),
            ..base(
                self,
                "Cooldown",
                "hourglass",
                "sync",
                "Short-circuits the run to success when a prior pass ran within the window; at most one pass proceeds per window.",
            )
        }
    }
}
