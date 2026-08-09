//! `percentage`: a weighted rollout, routing to a bucket by a stable hash of a key.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, control, end_ref, field, req};
use crate::node_kinds::{GraphRole, NodeKindSpec, TargetSlot};
use crate::parameters::parse_percentage_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Percentage;

impl NodeKindSpec for Percentage {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Percentage
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.handler_safe()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_percentage_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_percentage_parameters(node)?;
        let mut slots: Vec<TargetSlot> = params
            .buckets
            .into_iter()
            .map(|bucket| {
                TargetSlot::non_start("buckets", "percentage bucket target", bucket.target)
            })
            .collect();
        slots.extend(
            params
                .default
                .map(|target| TargetSlot::non_start("default", "percentage bucket target", target)),
        );
        Ok(slots)
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                req("key", RuninatorType::Any),
                FieldLocation::parameters(&["key"]),
                Some("expression"),
            )],
            edge_slots: vec![
                control("buckets", "Bucket", &["buckets"], true),
                control("default", "Percentage default", &["default"], false),
            ],
            default_template: json!({
                "kind": "percentage",
                "parameters": { "key": { "$ref": { "input": ["user_id"] } }, "buckets": [], "default": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Percentage",
                "percentage",
                "control-flow",
                "Weighted rollout: routes to a bucket by a stable hash of a key.",
            )
        }
    }
}
