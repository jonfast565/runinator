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

    /// the 1-based (line, column) of this span's start within `src`.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        let mut line = 1;
        let mut col = 1;
        for (idx, ch) in src.char_indices() {
            if idx >= self.start {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }
}

/// render a span against the source as a rustc-style caret snippet:
///
/// ```text
/// error: unknown field 'b' on 'input'
///  --> line 4, column 34
///   |
/// 4 |     console.run(command: input.b)
///   |                          ^^^^^^^
/// ```
pub(crate) fn render_snippet(src: &str, span: Span, label: &str, message: &str) -> String {
    let (line, column) = span.line_col(src);
    let line_text = src.lines().nth(line - 1).unwrap_or("");
    // clamp the underline to what remains on this line so multi-line spans stay tidy.
    let remaining = line_text.chars().count().saturating_sub(column - 1);
    let span_len = span.end.saturating_sub(span.start);
    let caret_len = span_len.min(remaining).max(1);
    let gutter = line.to_string();
    let pad = " ".repeat(gutter.len());
    let underline = format!("{}{}", " ".repeat(column - 1), "^".repeat(caret_len));
    format!(
        "{label}: {message}\n\
         {pad} --> line {line}, column {column}\n\
         {pad} |\n\
         {gutter} | {line_text}\n\
         {pad} | {underline}"
    )
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

    /// semantic analysis rejected the document (bad reference, type mismatch, scope error).
    #[error("semantic error at {}..{}: {message}", span.start, span.end)]
    Semantic { span: Span, message: String },

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

    pub(crate) fn semantic(span: Span, message: impl Into<String>) -> Self {
        Self::Semantic {
            span,
            message: message.into(),
        }
    }

    pub(crate) fn lower(message: impl Into<String>) -> Self {
        Self::Lower(message.into())
    }

    /// render this error against the source. span-carrying variants (`Syntax`, `Semantic`)
    /// become caret snippets; `Parse` keeps pest's already-rich rendering; the rest fall back
    /// to their `Display`.
    pub fn render(&self, src: &str) -> String {
        match self {
            Self::Syntax { span, message } | Self::Semantic { span, message } => {
                render_snippet(src, *span, "error", message)
            }
            other => other.to_string(),
        }
    }
}
