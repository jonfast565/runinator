#[allow(unused_imports)]
use super::*;

/// a header `type <Name> { ... }` (struct shorthand) or `type <Name> = <type>` (alias) declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
    pub comments: CommentSet,
}
