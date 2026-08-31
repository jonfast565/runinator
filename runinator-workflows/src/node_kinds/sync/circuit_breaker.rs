//! `circuit_breaker`: tracks failure rates across runs; fast-fails or routes to fallback when tripped.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use crate::node_kinds::builders::{
    base, end_ref, field, opt, positive_duration, positive_integer, req,
};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct CircuitBreaker;

impl NodeKindSpec for CircuitBreaker {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::CircuitBreaker
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("threshold", positive_integer()),
                    FieldLocation::parameters(&["threshold"]),
                    None,
                ),
                field(
                    opt("window_seconds", positive_duration()),
                    FieldLocation::parameters(&["window_seconds"]),
                    None,
                ),
                field(
                    opt("cooldown_seconds", positive_duration()),
                    FieldLocation::parameters(&["cooldown_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "circuit_breaker",
                "parameters": { "name": "my-circuit-breaker", "threshold": 5, "window_seconds": 60, "cooldown_seconds": 30 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Circuit Breaker",
                "shield",
                "sync",
                "Tracks failure rates across runs; fast-fails or routes to fallback when tripped.",
            )
        }
    }
}
