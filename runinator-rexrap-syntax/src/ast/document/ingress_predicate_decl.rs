#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct IngressPredicateDecl {
    pub pointer: String,
    pub operator: String,
    pub value: Option<Expr>,
    pub span: Span,
}
