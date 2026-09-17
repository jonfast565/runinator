#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ForStmt {
    pub var: String,
    /// optional item annotation used when source inference is too broad.
    pub var_type: Option<TypeExpr>,
    /// optional zero-based loop-position binding.
    pub index_var: Option<String>,
    pub items: Expr,
    /// iteration cap. `None` is uncapped (`limit none` or no clause). a literal
    /// integer lowers to the node's `max_iterations`; any other expression is
    /// carried in the loop parameters and resolved at runtime.
    pub limit: Option<Expr>,
    pub body: Block,
}
