#[allow(unused_imports)]
use super::*;

/// an ordered list of compute statements.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComputeProgram(pub Vec<ComputeStmt>);
