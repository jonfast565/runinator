#[allow(unused_imports)]
use super::*;

/// `assert { "name": cond, ... }`: named boolean invariants checked inline.
#[derive(Debug, Clone, PartialEq)]
pub struct AssertStmt {
    /// each entry is a (name, condition); the violation message defaults to the name.
    pub assertions: Vec<(String, Cond)>,
}
