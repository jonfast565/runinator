//! `condition`: routes down a branch based on a boolean expression.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{base, condition_branches, end_ref};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Condition;

impl NodeKindSpec for Condition {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Condition
    }

    fn graph_role(&self) -> GraphRole {
        // routes by predicate; the decision is the edge taken, not an addressable output.
        GraphRole::STEP.without_output()
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            edge_slots: vec![condition_branches()],
            default_template: json!({
                "kind": "condition", "condition": {},
                "transitions": {
                    "branches": [ { "when": { "value": { "$ref": { "params": ["approved"] } }, "equals": true }, "target": end_ref() } ],
                    "next": end_ref(),
                },
                "parameters": {}, "retry": { "max_attempts": 1 },
            }),
            ..base(
                self,
                "Condition",
                "branch",
                "control-flow",
                "Routes down a branch based on a boolean expression.",
            )
        }
    }
}
