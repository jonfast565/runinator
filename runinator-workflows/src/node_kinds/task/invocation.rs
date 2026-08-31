//! `invocation`: runs a compiled invocation program, suspending on each durable call.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorType as WorkflowType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use crate::node_kinds::builders::{base, field, opt, positive_duration};
use crate::node_kinds::{ActionCatalog, GraphRole, NodeKindSpec};
use crate::parameters::parse_invocation_parameters;
use runinator_compute::WorkflowValidationError;

pub(in crate::node_kinds) struct Invocation;

impl NodeKindSpec for Invocation {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Invocation
    }

    fn graph_role(&self) -> GraphRole {
        // the same role `action` carries, for the same reasons: it produces an addressable output,
        // a cursor on it is doing work and so may be interrupted, and it is handler-safe because
        // awaiting a worker is not the kind of parking that would pin a suspended thread open.
        GraphRole::STEP.handler_safe()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_invocation_parameters(node).map(|_| ())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        // a program's result type is whatever its last expression evaluates to, which is not
        // statically known from the node alone — the compiler records it as a type hint instead.
        // returning `None` means "unconstrained", not "null".
        Ok(None)
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("module", RuninatorType::Any),
                    FieldLocation::parameters(&["module"]),
                    None,
                ),
                field(
                    opt("timeout_seconds", positive_duration()),
                    FieldLocation::parameters(&["timeout_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "invocation",
                "parameters": { "module": { "version": 1, "entry": { "instructions": [] } } },
                "retry": { "max_attempts": 1 },
                "transitions": {},
            }),
            ..base(
                self,
                "Invocation",
                "code",
                "task",
                "Runs a compiled program, suspending on each durable call it makes.",
            )
        }
    }
}
