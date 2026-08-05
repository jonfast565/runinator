//! `await_run`: pauses until run(s) of a named workflow (optionally matching a correlation key) reach a terminal state.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, enum_ty, field, opt, req};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct AwaitRun;

impl NodeKindSpec for AwaitRun {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::AwaitRun
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("workflow", RuninatorType::String),
                    FieldLocation::parameters(&["workflow"]),
                    None,
                ),
                field(
                    opt("key", RuninatorType::Any),
                    FieldLocation::parameters(&["key"]),
                    Some("expression"),
                ),
                field(
                    opt("mode", enum_ty(&["all", "any"])),
                    FieldLocation::parameters(&["mode"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "await_run", "parameters": { "workflow": "", "mode": "all" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Await Workflow",
                "runs",
                "sync",
                "Pauses until run(s) of a named workflow (optionally matching a correlation key) reach a terminal state.",
            )
        }
    }
}
