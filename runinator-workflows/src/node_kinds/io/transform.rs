//! `transform`: resolves named expression bindings into the run context; no side effects.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, end_ref, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Transform;

impl NodeKindSpec for Transform {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Transform
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.handler_safe()
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                opt("bindings", RuninatorType::Any),
                FieldLocation::parameters(&["bindings"]),
                Some("json"),
            )],
            default_template: json!({
                "kind": "transform", "parameters": { "bindings": {} },
                "retry": { "max_attempts": 1 }, "transitions": { "next": end_ref() },
            }),
            ..base(
                self,
                "Transform",
                "gear",
                "io",
                "Resolves named expression bindings into the run context; no side effects.",
            )
        }
    }
}
