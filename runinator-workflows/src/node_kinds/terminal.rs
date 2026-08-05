//! the graph's entry and exit nodes.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::WorkflowNodeKind;

use super::builders::base;
use super::{GraphRole, NodeKindSpec};

pub(super) struct Start;
pub(super) struct End;
pub(super) struct Fail;

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

impl NodeKindSpec for End {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::End
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::TERMINAL
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            protected: true,
            addable: false,
            supports_predicate_edges: false,
            default_template: json!({ "kind": "end" }),
            ..base(
                self,
                "End",
                "flag",
                "terminal",
                "Terminal node that completes the run successfully.",
            )
        }
    }
}

impl NodeKindSpec for Fail {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Fail
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::TERMINAL
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            protected: true,
            addable: false,
            supports_predicate_edges: false,
            default_template: json!({ "kind": "fail" }),
            ..base(
                self,
                "Fail",
                "alert",
                "terminal",
                "Terminal node that ends the run as failed.",
            )
        }
    }
}
