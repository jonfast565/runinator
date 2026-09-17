#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileDecl {
    pub name: String,
    pub configuration: Expr,
    pub span: Span,
}
