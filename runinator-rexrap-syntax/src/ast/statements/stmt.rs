#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub span: Span,
    pub annotations: Annotations,
    /// `node <label> <- ...`; the binding doubles as the generated node id for leaf nodes.
    pub label: Option<String>,
    /// an optional `node <label>: <type> <- ...` annotation declaring the step's output type.
    pub label_type: Option<TypeExpr>,
    pub kind: StmtKind,
    /// `async <call>`: schedule this step as a task instead of joining it inline. Asyncness is a
    /// property of the call site, never of the callee, so no callable ever needs a second version.
    pub is_async: bool,
    pub transitions: TransitionClause,
    /// `compensate <call>` on an action node: the compensating action run in reverse on saga rollback.
    pub compensation: Option<Box<ActionStmt>>,
    /// leading/trailing/dangling comments, preserved for lossless formatting.
    pub comments: CommentSet,
}
