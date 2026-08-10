//! `interrupt`: begins an interrupt handler region, naming the source it answers.

use runinator_models::catalog_metadata::WorkflowNodeKindMetadata;
use runinator_models::json;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use runinator_compute::WorkflowValidationError;

use crate::node_kinds::builders::base;
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Interrupt;

impl NodeKindSpec for Interrupt {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Interrupt
    }

    /// the interrupt analogue of `start`: an entry point, so nothing may transition into it, but
    /// unlike `start` it is a legal region entry — the reducer places the handler cursor here.
    /// handler-safe because the region walk now starts at this node and every member must be.
    /// produces nothing addressable: it is a marker, not a step. not simulatable, because the
    /// dry-run walk starts at `start` and never reaches a region. not interruptible — interrupting
    /// the node that begins an interrupt is a knot with no upside.
    fn graph_role(&self) -> GraphRole {
        GraphRole {
            runnable_entry: true,
            entry_point: true,
            terminal: false,
            produces_output: false,
            reentrant: false,
            simulatable: false,
            handler_safe: true,
            interruptible: false,
        }
    }

    fn check_parameters(&self, _node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        Ok(())
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            // not addable: a bare interrupt node is not a valid graph on its own, so an editor
            // scaffolds the complete bounded region rather than asking for assembly node by node.
            // protected for the same reason `start` is — changing its kind orphans the region.
            protected: true,
            addable: false,
            supports_predicate_edges: false,
            fields: vec![],
            default_template: json!({ "kind": "interrupt" }),
            ..base(
                self,
                "Interrupt",
                "bolt",
                "terminal",
                "Entry point of an interrupt handler region for one source.",
            )
        }
    }
}
