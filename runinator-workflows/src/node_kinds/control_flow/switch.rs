//! `switch`: routes to one of several cases by matching a value.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, control, end_ref, field, req};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_switch_parameters;
use runinator_compute::WorkflowValidationError;
use runinator_compute::validate_condition;

pub(in crate::node_kinds) struct Switch;

impl NodeKindSpec for Switch {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Switch
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.handler_safe()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        for case in parse_switch_parameters(node)?.cases {
            validate_condition(&case.condition.to_value())?;
        }
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_switch_parameters(node)?;
        let mut slots: Vec<TargetSlot> = params
            .cases
            .into_iter()
            .map(|case| TargetSlot::non_entry("cases", "switch case target", case.target))
            .collect();
        slots.extend(
            params
                .default
                .map(|target| TargetSlot::non_entry("default", "switch default target", target)),
        );
        Ok(slots)
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                req("value", RuninatorType::Any),
                FieldLocation::parameters(&["value"]),
                Some("expression"),
            )],
            edge_slots: vec![
                control("cases", "Switch case", &["cases"], true),
                control("default", "Switch default", &["default"], false),
            ],
            default_template: json!({
                "kind": "switch",
                "parameters": { "value": { "$ref": { "params": ["mode"] } }, "cases": [], "default": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Switch",
                "switch",
                "control-flow",
                "Routes to one of several cases by matching a value.",
            )
        }
    }
}
