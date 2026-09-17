#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationBudgetDecl {
    pub name: String,
    pub attempts: u32,
    pub exhausted: String,
    pub handoff: Option<String>,
    pub span: Span,
}
