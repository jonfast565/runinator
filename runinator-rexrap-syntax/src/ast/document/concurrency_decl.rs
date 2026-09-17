#[allow(unused_imports)]
use super::*;

/// a header `concurrency <n> on_conflict <policy>` declaration. the policy defaults to `skip`:
/// writing a cap at all means the overlap is unwanted.
#[derive(Debug, Clone, PartialEq)]
pub struct ConcurrencyDecl {
    pub max_concurrent_runs: i64,
    pub on_conflict: ConcurrencyPolicy,
    pub span: Span,
    pub comments: CommentSet,
}
