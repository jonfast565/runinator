#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TransitionClause {
    pub next: Option<Target>,
    pub on_success: Option<Target>,
    pub on_failure: Option<Target>,
    pub on_timeout: Option<Target>,
    pub on_reject: Option<Target>,
    /// user-defined predicate edges, in declaration order; lowered to `transitions.branches`.
    pub branches: Vec<PredicateEdge>,
}

impl TransitionClause {
    pub fn is_empty(&self) -> bool {
        self.next.is_none()
            && self.on_success.is_none()
            && self.on_failure.is_none()
            && self.on_timeout.is_none()
            && self.on_reject.is_none()
            && self.branches.is_empty()
    }
}
