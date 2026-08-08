//! `race`: runs branches concurrently; the first to finish wins.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, control, enum_ty, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_race_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Race;

impl NodeKindSpec for Race {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Race
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.reentrant().not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_race_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(parse_race_parameters(node)?
            .branches
            .into_iter()
            .map(|branch| TargetSlot::runnable("branches", "race branch", branch))
            .collect())
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                opt("winner", enum_ty(&["all", "any", "first_success"])),
                FieldLocation::parameters(&["winner"]),
                None,
            )],
            edge_slots: vec![control("branches", "Race branch", &["branches"], true)],
            default_template: json!({
                "kind": "race", "parameters": { "branches": [] },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Race",
                "race",
                "concurrency",
                "Runs branches concurrently; the first to finish wins.",
            )
        }
    }
}
