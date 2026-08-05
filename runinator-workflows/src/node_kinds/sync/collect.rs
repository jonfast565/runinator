//! `collect`: accumulates externally-delivered items until a count or time threshold is met.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Collect;

impl NodeKindSpec for Collect {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Collect
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
                    opt("max", RuninatorType::Integer),
                    FieldLocation::parameters(&["max"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "collect", "parameters": { "name": "my-collect", "max": 10 },
                "retry": { "max_attempts": 1 }, "transitions": { "on_success": end_ref() },
            }),
            ..base(
                self,
                "Collect",
                "list",
                "sync",
                "Accumulates externally-delivered items until a count or time threshold is met.",
            )
        }
    }
}
