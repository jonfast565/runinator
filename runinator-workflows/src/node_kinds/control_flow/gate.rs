//! `gate`: blocks until an automated/policy check or manual gate opens.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, enum_ty, field, opt, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Gate;

impl NodeKindSpec for Gate {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Gate
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("kind", enum_ty(&["manual", "condition", "external"])),
                    FieldLocation::parameters(&["kind"]),
                    None,
                ),
                field(
                    opt("when", RuninatorType::Any),
                    FieldLocation::parameters(&["when"]),
                    Some("json"),
                ),
                field(
                    opt("poll_interval", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval"]),
                    None,
                ),
                field(
                    opt("timeout", RuninatorType::Integer),
                    FieldLocation::parameters(&["timeout"]),
                    None,
                ),
                field(
                    opt("timeout_policy", enum_ty(&["fail", "continue"])),
                    FieldLocation::parameters(&["timeout_policy"]),
                    None,
                ),
                field(
                    opt("label", RuninatorType::String),
                    FieldLocation::parameters(&["label"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "gate",
                "parameters": { "kind": "manual", "poll_interval": 30 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Gate",
                "shield",
                "control-flow",
                "Blocks until an automated/policy check or manual gate opens.",
            )
        }
    }
}
