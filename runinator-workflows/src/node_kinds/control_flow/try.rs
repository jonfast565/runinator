//! `try`: guards a body node and catches failures with a handler.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::errors::WorkflowValidationError;
use crate::node_kinds::builders::{base, control, end_ref};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_try_parameters;

pub(in crate::node_kinds) struct Try;

impl NodeKindSpec for Try {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Try
    }

    fn graph_role(&self) -> GraphRole {
        // the guarded body routes back here to settle, and the simulator does not model the frame.
        GraphRole::STEP
            .without_output()
            .reentrant()
            .not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_try_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_try_parameters(node)?;
        let mut slots = vec![TargetSlot::runnable("body", "try body", params.body)];
        slots.extend(
            params
                .catch
                .map(|target| TargetSlot::runnable("catch", "try catch", target)),
        );
        slots.extend(
            params
                .finally
                .map(|target| TargetSlot::runnable("finally", "try finally", target)),
        );
        Ok(slots)
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            edge_slots: vec![
                control("body", "Try body", &["body"], false),
                control("catch", "Try catch", &["catch"], false),
                control("finally", "Try finally", &["finally"], false),
            ],
            default_template: json!({
                "kind": "try",
                "parameters": { "body": end_ref(), "catch": end_ref(), "finally": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Try",
                "shield",
                "control-flow",
                "Guards a body node and catches failures with a handler.",
            )
        }
    }
}
