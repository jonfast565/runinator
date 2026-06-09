// semantic analysis over the wdl ast. runs after parsing and before lowering so every
// diagnostic can anchor to a source span. four passes contribute, in order: name/reference
// resolution, scope correctness, type checking, and reachability. errors block compilation;
// warnings are advisory and surfaced through `compile_str_with_diagnostics`.
//
// diagnostics anchor at expression granularity: `Expr`/`Cond` carry their own spans, so the
// offending sub-expression (a bad operand, a missing field, an unknown reference) is the unit
// of blame. statement-level findings (duplicate/reserved ids, transition targets, reachability)
// still use the statement span. the one remaining coarseness is per-`PathSeg`: a whole dotted
// path shares one span, so `input.b.c` blames the path, not the `b` segment — a future refinement.

mod functions;
mod reachability;
mod scope;
mod types;

use crate::ast::{Block, Document, Stmt, StmtKind};
use crate::errors::Span;

/// whether a diagnostic blocks compilation or is merely advisory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// a single semantic finding anchored to a source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub(crate) fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            severity: Severity::Error,
            message: message.into(),
        }
    }

    pub(crate) fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            severity: Severity::Warning,
            message: message.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// render this diagnostic against the source as a caret snippet.
    pub fn render(&self, src: &str) -> String {
        let label = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        crate::errors::render_snippet(src, self.span, label, &self.message)
    }
}

/// run every semantic pass and collect their diagnostics in priority order.
pub fn analyze(document: &Document) -> Vec<Diagnostic> {
    let workflow = &document.workflow;
    let mut diagnostics = Vec::new();

    // pass 0: validate top-level `fn` definitions (duplicates, pure bodies, recursion annotations).
    functions::analyze(&document.functions, &mut diagnostics);
    // pass 1+2: build the label table, then resolve references and scopes against it.
    scope::analyze(workflow, &document.functions, &mut diagnostics);
    // pass 3: type-check expressions, conditions, and `let` annotations.
    types::analyze(workflow, &mut diagnostics);
    // pass 4: flag structurally unreachable statements (warnings only).
    reachability::analyze(workflow, &mut diagnostics);

    diagnostics
}

/// the first error-severity diagnostic, if any. warnings are ignored.
pub fn first_error(diagnostics: &[Diagnostic]) -> Option<&Diagnostic> {
    diagnostics.iter().find(|diagnostic| diagnostic.is_error())
}

/// the referenceable node id for a statement: an explicit id wins over the let label.
pub(super) fn effective_id(stmt: &Stmt) -> Option<&str> {
    stmt.annotations.id.as_deref().or(stmt.label.as_deref())
}

/// the nested statement blocks owned by a control statement (empty for leaves).
pub(super) fn child_blocks(kind: &StmtKind) -> Vec<&Block> {
    match kind {
        StmtKind::If(if_stmt) => {
            let mut blocks: Vec<&Block> = if_stmt.arms.iter().map(|(_, body)| body).collect();
            if let Some(else_block) = &if_stmt.else_block {
                blocks.push(else_block);
            }
            blocks
        }
        StmtKind::For(for_stmt) => vec![&for_stmt.body],
        StmtKind::While(while_stmt) => vec![&while_stmt.body],
        StmtKind::Map(map_stmt) => vec![&map_stmt.body],
        StmtKind::Match(match_stmt) => {
            let mut blocks: Vec<&Block> = match_stmt.arms.iter().map(|arm| &arm.body).collect();
            if let Some(default) = &match_stmt.default {
                blocks.push(default);
            }
            blocks
        }
        StmtKind::Parallel(parallel) => parallel.branches.iter().collect(),
        StmtKind::Race(race) => race.branches.iter().collect(),
        StmtKind::Try(try_stmt) => {
            let mut blocks = vec![&try_stmt.body];
            if let Some(catch) = &try_stmt.catch {
                blocks.push(catch);
            }
            if let Some(finally) = &try_stmt.finally {
                blocks.push(finally);
            }
            blocks
        }
        _ => Vec::new(),
    }
}
