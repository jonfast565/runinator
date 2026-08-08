//! `join`: waits for upstream branches to finish before continuing.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, control, enum_ty, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_join_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Join;

impl NodeKindSpec for Join {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Join
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_join_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(parse_join_parameters(node)?
            .wait_for
            .into_iter()
            .map(|target| TargetSlot::runnable("wait_for", "join wait_for", target))
            .collect())
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                opt("mode", enum_ty(&["all", "any", "first_success"])),
                FieldLocation::parameters(&["mode"]),
                None,
            )],
            edge_slots: vec![control("wait_for", "Join dependency", &["wait_for"], true)],
            default_template: json!({
                "kind": "join", "parameters": { "wait_for": [], "mode": "all" },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Join",
                "join",
                "concurrency",
                "Waits for upstream branches to finish before continuing.",
            )
        }
    }
}
