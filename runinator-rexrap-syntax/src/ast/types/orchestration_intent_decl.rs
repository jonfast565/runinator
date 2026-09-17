#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OrchestrationIntentDecl {
    pub name: String,
    pub effect: String,
    pub priority: i32,
    pub coalesce_seconds: Option<u64>,
    pub stop: Option<String>,
    pub restart: Option<String>,
    pub revision: Option<String>,
    pub signal_name: Option<String>,
    pub allow_self_originated: bool,
    pub span: Span,
}
