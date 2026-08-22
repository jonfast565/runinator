//! `resume`: ends an interrupt handler region and hands control back to the suspended thread.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::interrupt::InterruptMode;
use runinator_models::json;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use runinator_compute::WorkflowValidationError;

use crate::node_kinds::builders::{base, enum_ty, field, opt};
use crate::node_kinds::{GraphRole, NodeKindSpec};

pub(in crate::node_kinds) struct Resume;

/// the author-facing mode names, in the order the UI should offer them. read off
/// [`InterruptMode::ALL`] so this field, the catalog's `resume_mode` enum, and the runtime cannot
/// disagree about which modes exist.
fn modes() -> Vec<&'static str> {
    InterruptMode::ALL
        .iter()
        .map(|mode| mode.as_str())
        .collect()
}

impl NodeKindSpec for Resume {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Resume
    }

    /// terminal for its own thread of control, but unlike `end`/`fail` it must be a legal region
    /// entry: a one-statement handler is just a bare `resume`, and that node is the region's start.
    /// not interruptible — interrupting the node that ends an interrupt is a knot with no upside.
    /// not simulatable, because the dry-run walk has no interrupted thread to hand control back to.
    fn graph_role(&self) -> GraphRole {
        GraphRole {
            runnable_entry: true,
            entry_point: false,
            terminal: true,
            produces_output: false,
            reentrant: false,
            simulatable: false,
            handler_safe: true,
            interruptible: false,
        }
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        let Some(mode) = node.parameters.get("mode") else {
            // absent means the default, a plain resume.
            return Ok(());
        };
        let named =
            mode.as_str()
                .ok_or_else(|| WorkflowValidationError::InvalidNodeParameters {
                    node: node.id.clone(),
                    message: "resume mode must be a string".into(),
                })?;
        named.parse::<InterruptMode>().map(|_| ()).map_err(|_| {
            WorkflowValidationError::InvalidNodeParameters {
                node: node.id.clone(),
                message: format!(
                    "unknown resume mode '{named}'; expected one of {}",
                    modes().join(", ")
                ),
            }
        })
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            // addable, but only lands in a handler region — region validation is what enforces that,
            // since "is this node inside a handler" is a graph property the palette cannot see.
            supports_predicate_edges: false,
            fields: vec![field(
                opt("mode", enum_ty(&modes())),
                FieldLocation::parameters(&["mode"]),
                None,
            )],
            default_template: json!({
                "kind": "resume",
                "parameters": { "mode": "resume" },
            }),
            ..base(
                self,
                "Resume",
                "flag",
                "control-flow",
                "Ends an interrupt handler and returns control to the interrupted thread.",
            )
        }
    }
}
