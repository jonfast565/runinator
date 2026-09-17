#[allow(unused_imports)]
use super::*;

/// a header `import [kind] <path> [@revision(N)] (as <alias>)?` declaration. `path` is the
/// dotted namespace (`std.strings`, `acme.billing.reconcile`); `alias` binds a short local name.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    pub kind: Option<ImportKind>,
    pub path: String,
    /// `@revision(N)` is meaningful for workflow imports. It is resolved to a UUID + digest by
    /// pack import, never treated as a runtime name lookup.
    pub revision: Option<i64>,
    pub alias: Option<String>,
    pub span: Span,
    pub comments: CommentSet,
}
