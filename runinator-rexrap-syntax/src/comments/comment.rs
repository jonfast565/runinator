#[allow(unused_imports)]
use super::*;

/// a single source comment with its byte span and verbatim text (delimiters included).
#[derive(Debug, Clone, PartialEq)]
pub struct Comment {
    pub kind: CommentKind,
    /// the exact source text, e.g. `// note` or `/* note */`.
    pub text: String,
    pub span: Span,
    /// true when only whitespace precedes the comment on its line (so it renders on its own line).
    pub own_line: bool,
}
