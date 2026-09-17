#[allow(unused_imports)]
use super::*;

/// a user-defined predicate edge: take `target` when `when` holds. `priority` orders evaluation
/// among predicate edges (lower first); `None` keeps declaration order.
#[derive(Debug, Clone, PartialEq)]
pub struct PredicateEdge {
    pub when: Cond,
    pub target: Target,
    pub priority: Option<i64>,
}
