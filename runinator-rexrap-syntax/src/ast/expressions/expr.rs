#[allow(unused_imports)]
use super::*;

/// an expression paired with the source span it was parsed from, so diagnostics can
/// anchor to the offending sub-expression rather than the enclosing statement.
#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Self { kind, span }
    }
}
