#[allow(unused_imports)]
use super::*;

/// a top-level `fn name(params) -> ret = body` definition. the body is either a single expression
/// or a compute-style statement block; `recursive` carries the `@recursive(max_depth: N)` cap when
/// present.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    /// `task fn` — the body contains runtime work and is inlined into the graph. Callers still
    /// choose the scheduling (`f(...)` vs `async f(...)`), so this is a capability, not a color.
    pub is_task: bool,
    pub params: Vec<FnParam>,
    pub ret: Option<TypeExpr>,
    pub body: FnBody,
    pub recursive: Option<u32>,
    pub span: Span,
    /// leading/trailing comments, preserved for lossless formatting.
    pub comments: CommentSet,
}
