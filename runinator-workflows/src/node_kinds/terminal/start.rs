//! `start`: entry point where the workflow run begins.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::base;
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Start;

impl NodeKindSpec for Start {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Start
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::START
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            protected: true,
            addable: false,
            supports_predicate_edges: false,
            default_template: json!({ "kind": "start", "transitions": {} }),
            ..base(
                self,
                "Start",
                "play",
                "terminal",
                "Entry point where the workflow run begins.",
            )
        }
    }
}
