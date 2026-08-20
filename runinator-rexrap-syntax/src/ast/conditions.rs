use super::*;

// conditions ----------------------------------------------------------------

/// a condition paired with the source span it was parsed from.
#[derive(Debug, Clone, PartialEq)]
pub struct Cond {
    pub kind: CondKind,
    pub span: Span,
}

impl Cond {
    pub fn new(kind: CondKind, span: Span) -> Self {
        Self { kind, span }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CondKind {
    All(Vec<Cond>),
    Any(Vec<Cond>),
    Not(Box<Cond>),
    Expr(Expr),
    Cmp { left: Expr, op: CmpOp, right: Expr },
    Exists(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    In,
    StartsWith,
    EndsWith,
}
