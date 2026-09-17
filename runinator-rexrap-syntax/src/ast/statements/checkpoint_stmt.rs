#[allow(unused_imports)]
use super::*;

/// `checkpoint "name"`: a named state snapshot for later rollback.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckpointStmt {
    pub name: String,
}
