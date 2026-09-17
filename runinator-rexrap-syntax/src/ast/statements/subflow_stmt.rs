#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct SubflowStmt {
    pub workflow_name: String,
    /// A workflow-import revision selector, carried through codegen as a temporary reference until
    /// the importer replaces it with an exact UUID/digest pin.
    pub revision: Option<i64>,
    /// Set by namespace resolution when the target came from a typed workflow import. Its
    /// signature is supplied by the package resolver, so an offline authoring compile may retain
    /// an `Any` interface rather than rejecting a path it cannot inspect locally.
    pub imported: bool,
    /// `detached: true` => fire-and-forget; otherwise wait.
    pub detached: bool,
    pub reuse: bool,
    pub run_name: Option<Expr>,
    pub params: Vec<(String, Expr)>,
}
