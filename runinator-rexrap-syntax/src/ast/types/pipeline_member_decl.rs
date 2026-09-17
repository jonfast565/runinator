#[allow(unused_imports)]
use super::*;

/// a `workflow "Name"` member declaration, optionally followed by `on_failure <mode>`. `on_failure`
/// holds the raw keyword (`stop`/`continue`/`silently_continue`/`inquire`) or `None` when the member
/// takes the pipeline's default failure mode; lowering maps it to [`PipelineMemberFailureMode`]
/// (`runinator_models::pipelines`).
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineMemberDecl {
    pub workspace: Option<super::super::Expr>,
    pub name: String,
    pub on_failure: Option<String>,
    pub span: Span,
}
