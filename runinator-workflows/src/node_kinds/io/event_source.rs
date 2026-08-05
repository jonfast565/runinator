//! `event_source`: subscribes to a named event stream; drives a body subgraph on each matching event.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct EventSource;

impl NodeKindSpec for EventSource {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::EventSource
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("event_type", RuninatorType::String),
                    FieldLocation::parameters(&["event_type"]),
                    None,
                ),
                field(
                    opt("filter", RuninatorType::Any),
                    FieldLocation::parameters(&["filter"]),
                    Some("expression"),
                ),
                field(
                    opt("max", RuninatorType::Integer),
                    FieldLocation::parameters(&["max"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "event_source", "parameters": { "event_type": "" },
                "retry": { "max_attempts": 1 }, "transitions": { "on_success": end_ref() },
            }),
            ..base(
                self,
                "Event Source",
                "bell",
                "io",
                "Subscribes to a named event stream; drives a body subgraph on each matching event.",
            )
        }
    }
}
