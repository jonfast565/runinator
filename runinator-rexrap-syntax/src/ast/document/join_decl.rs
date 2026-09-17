#[allow(unused_imports)]
use super::*;

/// a `join <name> { … }` named continuation: a labelled region a `continue <name>` route enters.
/// unlike a fallthrough sibling it is never reached implicitly.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinDecl {
    pub name: String,
    pub body: Block,
    pub span: Span,
    pub comments: CommentSet,
}
