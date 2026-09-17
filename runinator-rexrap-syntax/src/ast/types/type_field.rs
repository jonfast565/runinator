#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct TypeField {
    pub name: String,
    pub optional: bool,
    pub ty: TypeExpr,
    /// an optional default expression, only present on top-level workflow parameter fields. when
    /// set the field is effectively optional and the expression fills it at run start if omitted.
    pub default: Option<Expr>,
    /// the source span of this field, used to attach comments for lossless formatting. defaults to an
    /// empty span for fields synthesized outside the parser.
    pub span: Span,
    /// leading/trailing/dangling comments on this `params`/`type` struct field.
    pub comments: CommentSet,
}
