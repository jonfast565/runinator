#[allow(unused_imports)]
use super::*;

/// one node target read out of a node's parameters.
#[derive(Debug, Clone)]
pub struct TargetSlot {
    /// the catalog edge-slot key this target came from; a conformance test pins the two together.
    pub key: &'static str,
    /// how the target is described in a validation error ("switch case target").
    pub label: &'static str,
    /// what the target is allowed to be.
    pub rule: TargetRule,
    /// the referenced node.
    pub target: WorkflowNodeRef,
}

impl TargetSlot {
    /// a routing target: anything that is not an entry point.
    pub fn non_entry(key: &'static str, label: &'static str, target: WorkflowNodeRef) -> Self {
        Self {
            key,
            label,
            rule: TargetRule::NonEntry,
            target,
        }
    }

    /// the entry of a body or branch region: a runnable, non-terminal node.
    pub fn runnable(key: &'static str, label: &'static str, target: WorkflowNodeRef) -> Self {
        Self {
            key,
            label,
            rule: TargetRule::RunnableEntry,
            target,
        }
    }
}
