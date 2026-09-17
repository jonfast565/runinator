#[allow(unused_imports)]
use super::*;

/// a weighted, hash-bucketed router: `hash(key) % total_weight` selects a bucket. sticky per key.
#[derive(Debug, Clone, PartialEq)]
pub struct PercentageParameters {
    pub key: WorkflowExpression,
    pub buckets: Vec<PercentageBucket>,
    pub default: Option<WorkflowNodeRef>,
}
