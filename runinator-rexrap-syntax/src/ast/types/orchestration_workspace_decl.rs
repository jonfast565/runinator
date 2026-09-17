#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationWorkspaceDecl {
    pub scope: String,
    pub reuse: bool,
    pub lease_seconds: Option<u64>,
    pub recovery: Option<String>,
    pub labels: Option<Expr>,
    pub span: Span,
}
