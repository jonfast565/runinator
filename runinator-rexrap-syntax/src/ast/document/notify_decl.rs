#[allow(unused_imports)]
use super::*;

/// a header `notify on <event> -> <channel> <target>` declaration. `after` carries the threshold
/// seconds for the duration events; `severity` defaults to `warning` when omitted.
#[derive(Debug, Clone, PartialEq)]
pub struct NotifyDecl {
    pub event: NotifyEvent,
    pub channel: NotifyChannel,
    pub target: Expr,
    pub after_seconds: Option<i64>,
    pub severity: Option<String>,
    /// optional `with { ... }` provider configuration overriding the generated delivery fields.
    pub configuration: Option<Expr>,
    /// expose actions for the currently parked run effect.
    pub interactive: bool,
    pub enabled: bool,
    pub span: Span,
    pub comments: CommentSet,
}
