#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineJoinDecl {
    pub target: String,
    pub mode: String,
    pub parameters: Option<Expr>,
    pub span: Span,
}
