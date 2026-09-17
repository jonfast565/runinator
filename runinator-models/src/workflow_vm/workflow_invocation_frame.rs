#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowInvocationFrame {
    pub module: InvocationModule,
    pub continuation: crate::invocation::InvocationContinuation,
}
