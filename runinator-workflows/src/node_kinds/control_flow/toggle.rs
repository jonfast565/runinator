//! `toggle`: a light switch, routing to on or off by a value's truthiness.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, control, end_ref, field, req};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_toggle_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Toggle;

impl NodeKindSpec for Toggle {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Toggle
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.handler_safe()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_toggle_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_toggle_parameters(node)?;
        Ok(vec![
            TargetSlot::non_start("on", "toggle on/off target", params.on),
            TargetSlot::non_start("off", "toggle on/off target", params.off),
        ])
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
                control("on", "Toggle on", &["on"], false),
                control("off", "Toggle off", &["off"], false),
            ],
            default_template: json!({
                "kind": "toggle",
                "parameters": { "value": { "$ref": { "config": ["flags", "enabled"] } }, "on": end_ref(), "off": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Toggle",
                "toggle",
                "control-flow",
                "A light switch: routes to on or off by a value's truthiness.",
            )
        }
    }
}
