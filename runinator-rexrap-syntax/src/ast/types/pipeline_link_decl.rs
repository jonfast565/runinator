#[allow(unused_imports)]
use super::*;

/// a directed link in a `.rexrapp` pipeline: `"A" -> "B" on <selector>`. `on` holds the raw selector
/// keyword (`success`/`complete`/`failure`) or `None` when omitted; lowering resolves it.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineLinkDecl {
    pub from: String,
    pub to: String,
    pub on: Option<String>,
    pub disabled: bool,
    pub parameters: Option<Expr>,
    pub span: Span,
}
