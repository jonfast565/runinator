#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationDecl {
    pub intents: Vec<OrchestrationIntentDecl>,
    pub budgets: Vec<OrchestrationBudgetDecl>,
    pub entry_member: Option<String>,
    pub max_epochs: Option<u32>,
    pub defaults: Option<Expr>,
    pub phases: Vec<OrchestrationPhaseDecl>,
    pub span: Span,
}
