use runinator_models::errors::{EngineErrors, ErrorDescriptor};
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
/// error: unknown field 'b' on 'params'
///  --> line 4, column 34
///   |
/// 4 |     console.run(command: params.b)
///   |                          ^^^^^^^
/// ```
pub fn render_snippet(src: &str, span: Span, label: &str, message: &str) -> String {
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

/// errors produced while compiling or decompiling rexrap.
#[derive(Debug, Error)]
pub enum RexRapError {
    /// the grammar rejected the source. carries pest's rendered message, and the position it
    /// rejected at when the parser could report one. without the span the caret renderer had
    /// nothing to anchor to, so the single most common authoring error was the one diagnostic that
    /// underlined the whole file instead of the offending token.
    #[error("REXRAP001 - parse error:\n{message}")]
    Parse { span: Option<Span>, message: String },

    /// the parse tree was structurally valid but semantically malformed.
    #[error("REXRAP002 - syntax error at {}..{}: {message}", span.start, span.end)]
    Syntax { span: Span, message: String },

    /// semantic analysis rejected the document (bad reference, type mismatch, scope error).
    #[error("REXRAP003 - semantic error at {}..{}: {message}", span.start, span.end)]
    Semantic { span: Span, message: String },

    /// lowering the ast to the json model failed.
    #[error("REXRAP004 - lowering error: {0}")]
    Lower(String),

    /// the lowered definition failed the shared workflow validator.
    #[error("REXRAP005 - validation error: {0}")]
    Validation(String),

    /// decompiling a json definition back to rexrap failed.
    #[error("REXRAP006 - decompile error: {0}")]
    Decompile(String),
}

// numbered error dictionary for the rexrap surface language.
pub const PARSE: ErrorDescriptor = ErrorDescriptor::new("REXRAP001", "rexrap.parse", "Parse error");
pub const SYNTAX: ErrorDescriptor =
    ErrorDescriptor::new("REXRAP002", "rexrap.syntax", "Syntax error");
pub const SEMANTIC: ErrorDescriptor =
    ErrorDescriptor::new("REXRAP003", "rexrap.semantic", "Semantic error");
pub const LOWER: ErrorDescriptor =
    ErrorDescriptor::new("REXRAP004", "rexrap.lower", "Lowering error");
pub const VALIDATION: ErrorDescriptor =
    ErrorDescriptor::new("REXRAP005", "rexrap.validation", "Validation error");
pub const DECOMPILE: ErrorDescriptor =
    ErrorDescriptor::new("REXRAP006", "rexrap.decompile", "Decompile error");

pub const DICTIONARY: &[ErrorDescriptor] = &[PARSE, SYNTAX, SEMANTIC, LOWER, VALIDATION, DECOMPILE];

impl EngineErrors for RexRapError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}

impl RexRapError {
    pub fn syntax(span: Span, message: impl Into<String>) -> Self {
        Self::Syntax {
            span,
            message: message.into(),
        }
    }

    pub fn semantic(span: Span, message: impl Into<String>) -> Self {
        Self::Semantic {
            span,
            message: message.into(),
        }
    }

    pub fn lower(message: impl Into<String>) -> Self {
        Self::Lower(message.into())
    }

    /// a parse failure with no position: the parser could not say where.
    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse {
            span: None,
            message: message.into(),
        }
    }

    /// a parse failure anchored at the position the grammar rejected.
    pub fn parse_at(span: Span, message: impl Into<String>) -> Self {
        Self::Parse {
            span: Some(span),
            message: message.into(),
        }
    }

    /// the source position this error points at, for callers that anchor their own diagnostics.
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::Syntax { span, .. } | Self::Semantic { span, .. } => Some(*span),
            Self::Parse { span, .. } => *span,
            _ => None,
        }
    }

    /// render this error against the source. span-carrying variants become caret snippets; the
    /// rest fall back to their `Display`.
    pub fn render(&self, src: &str) -> String {
        match self {
            Self::Parse {
                span: Some(span),
                message,
            } => render_snippet(
                src,
                *span,
                "error",
                &format!("{} - {}", PARSE.code, parse_summary(message)),
            ),
            Self::Syntax { span, message } => {
                render_snippet(src, *span, "error", &format!("{} - {message}", SYNTAX.code))
            }
            Self::Semantic { span, message } => render_snippet(
                src,
                *span,
                "error",
                &format!("{} - {message}", SEMANTIC.code),
            ),
            other => other.to_string(),
        }
    }
}

/// pest renders its own multi-line caret block, which would sit oddly inside another one. keep
/// the expectation line, which is the part that says what the grammar wanted; the position it also
/// reports is already carried by the span.
fn parse_summary(message: &str) -> &str {
    message
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("= "))
        .or_else(|| {
            message
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with("-->") && !line.starts_with('|'))
                .next_back()
        })
        .unwrap_or(message)
}
