#[allow(unused_imports)]
use super::*;

/// an imperative `do { ... }` block. lowers to a `std.run`/`std.exec` action node.
#[derive(Debug, Clone, PartialEq)]
pub struct ComputeStmt {
    pub body: Vec<ComputeLine>,
    pub foreign: Option<ForeignDo>,
    pub modifiers: Modifiers,
}
