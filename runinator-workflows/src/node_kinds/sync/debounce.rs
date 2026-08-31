//! `debounce`: parks with a trailing delay that resets on re-trigger; collapses event bursts.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt, positive_duration, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Debounce;

impl NodeKindSpec for Debounce {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Debounce
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
                    opt("delay_seconds", positive_duration()),
                    FieldLocation::parameters(&["delay_seconds"]),
                    None,
                ),
                field(
                    opt("trigger_key", RuninatorType::Any),
                    FieldLocation::parameters(&["trigger_key"]),
                    Some("expression"),
                ),
            ],
            default_template: json!({
                "kind": "debounce", "parameters": { "name": "my-debounce", "delay_seconds": 30 },
                "retry": { "max_attempts": 1 }, "transitions": { "on_success": end_ref() },
            }),
            ..base(
                self,
                "Debounce",
                "clock",
                "sync",
                "Parks with a trailing delay that resets on re-trigger; collapses event bursts.",
            )
        }
    }
}
