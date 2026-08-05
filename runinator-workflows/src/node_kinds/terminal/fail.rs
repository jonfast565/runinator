//! `fail`: terminal node that ends the run as failed.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::base;
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Fail;

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
