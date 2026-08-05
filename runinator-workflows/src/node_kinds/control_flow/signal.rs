//! `signal`: pauses until a named external signal is delivered to the run.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Signal;

impl NodeKindSpec for Signal {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Signal
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
                "kind": "signal",
                "parameters": { "name": "signal" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Signal",
                "bell",
                "control-flow",
                "Pauses until a named external signal is delivered to the run.",
            )
        }
    }
}
