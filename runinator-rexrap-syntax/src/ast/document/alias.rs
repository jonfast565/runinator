#[allow(unused_imports)]
use super::*;

/// a header `alias <name> = { k: expr, ... }` binding: a named, reusable group of argument
/// values spread into action calls with `...name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    pub name: String,
    pub entries: Vec<(String, Expr)>,
    pub span: Span,
    pub comments: CommentSet,
}
