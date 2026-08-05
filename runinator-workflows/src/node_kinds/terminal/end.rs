//! `end`: terminal node that completes the run successfully.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::base;
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct End;

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
