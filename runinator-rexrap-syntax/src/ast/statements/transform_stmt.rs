#[allow(unused_imports)]
use super::*;

/// `transform { name = expr, ... }`: named context bindings reshaped from the runtime context.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformStmt {
    pub bindings: Vec<(String, Expr)>,
}
