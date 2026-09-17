#[allow(unused_imports)]
use super::*;

/// a header `trigger ...` declaration. `params` is the optional run parameter object shared by both
/// kinds; `kind` carries the cron schedule or the chaining target.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerDecl {
    pub kind: TriggerDeclKind,
    pub params: Option<Expr>,
    pub enabled: bool,
    pub span: Span,
    pub comments: CommentSet,
}
