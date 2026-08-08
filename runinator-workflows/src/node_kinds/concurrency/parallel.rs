//! `parallel`: fans out into branches that run concurrently.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, control};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_parallel_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Parallel;

impl NodeKindSpec for Parallel {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Parallel
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_parallel_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(parse_parallel_parameters(node)?
            .branches
            .into_iter()
            .map(|branch| TargetSlot::runnable("branches", "parallel branch", branch))
            .collect())
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            edge_slots: vec![control("branches", "Parallel branch", &["branches"], true)],
            default_template: json!({
                "kind": "parallel", "parameters": { "branches": [] },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Parallel",
                "parallel",
                "concurrency",
                "Fans out into branches that run concurrently.",
            )
        }
    }
}
