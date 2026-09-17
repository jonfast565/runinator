#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct SourceModule {
    pub path: String,
    pub functions: Vec<FunctionDef>,
    pub span: Span,
}
