#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct GateParameters {
    pub kind: GateKind,
    pub condition: WorkflowCondition,
    pub poll_interval_seconds: i64,
    pub deadline_seconds: Option<i64>,
    pub timeout_policy: GateTimeoutPolicy,
    pub label: Option<String>,
    pub metadata: Value,
}
