use thiserror::Error;

/// a byte span into the source text, used to anchor diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// errors produced while compiling or decompiling wdl.
#[derive(Debug, Error)]
pub enum WdlError {
    /// the grammar rejected the source. carries pest's rendered message.
    #[error("parse error:\n{0}")]
    Parse(String),

    /// the parse tree was structurally valid but semantically malformed.
    #[error("syntax error at {}..{}: {message}", span.start, span.end)]
    Syntax { span: Span, message: String },

    /// lowering the ast to the json model failed.
    #[error("lowering error: {0}")]
    Lower(String),

    /// the lowered definition failed the shared workflow validator.
    #[error("validation error: {0}")]
    Validation(String),

    /// decompiling a json definition back to wdl failed.
    #[error("decompile error: {0}")]
    Decompile(String),
}

impl WdlError {
    pub(crate) fn syntax(span: Span, message: impl Into<String>) -> Self {
        Self::Syntax {
            span,
            message: message.into(),
        }
    }

    pub(crate) fn lower(message: impl Into<String>) -> Self {
        Self::Lower(message.into())
    }
}
