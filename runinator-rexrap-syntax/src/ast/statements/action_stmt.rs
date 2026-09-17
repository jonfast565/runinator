#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ActionStmt {
    pub provider: String,
    pub function: String,
    /// argument entries in source order. a `...alias` spread is carried as an entry whose value
    /// is `ExprKind::Spread`; desugaring expands it in place before sema and lowering.
    pub args: Vec<(String, Expr)>,
    pub modifiers: Modifiers,
}
