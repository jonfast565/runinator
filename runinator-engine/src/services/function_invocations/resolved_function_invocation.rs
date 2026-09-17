#[allow(unused_imports)]
use super::*;

pub struct ResolvedFunctionInvocation {
    pub version: FunctionVersion,
    pub export: FunctionExport,
    pub workflow_id: Uuid,
}
