#[allow(unused_imports)]
use super::*;

/// what a running invocation is told about itself.
///
/// passed into the container so packaged code can log, correlate, and emit artifacts against the
/// right run without being handed the control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionInvocationContext {
    pub package: String,
    pub export: String,
    pub version: i64,
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub attempt: i64,
}
