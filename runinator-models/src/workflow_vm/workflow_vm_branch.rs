#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowVmBranch {
    pub condition: WorkflowCondition,
    pub target: usize,
}
