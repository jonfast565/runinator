//! deciding whether a cell can be answered in process or needs a run.

use runinator_models::value::Value;
use runinator_rexrap::{CompileOptions, RexRapFragmentKind};

use crate::errors::{ConsoleError, Result};

/// what a cell turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellKind {
    /// a single expression, evaluable in process against the session's bindings.
    Expression,
    /// a `do` program: several pure statements, still evaluable in process.
    Do,
    /// anything effectful or structural — an action call, control flow, several statements. it
    /// becomes a scratch workflow and goes through the reducer.
    Workflow,
}

/// a classified cell plus what the caller needs to act on it.
#[derive(Debug, Clone)]
pub struct Classification {
    pub kind: CellKind,
    /// the lowered fragment, for a pure cell. `None` for a workflow cell.
    pub lowered: Option<Value>,
    /// the rexrap source to compile, for a workflow cell. `None` for a pure cell.
    pub workflow_source: Option<String>,
}

impl Classification {
    /// true when this cell can be answered without starting a run.
    pub fn is_pure(&self) -> bool {
        matches!(self.kind, CellKind::Expression | CellKind::Do)
    }

    /// the fragment kind a pure cell evaluates as.
    pub fn fragment_kind(&self) -> Option<RexRapFragmentKind> {
        match self.kind {
            CellKind::Expression => Some(RexRapFragmentKind::Expression),
            CellKind::Do => Some(RexRapFragmentKind::Do),
            CellKind::Workflow => None,
        }
    }
}

/// classify one cell's source.
///
/// the order is the whole design. an expression is tried first because it is the cheapest and most
/// common cell; a `do` block second; and *anything else* becomes a workflow. the fallback is
/// deliberately last and unconditional — a cell this cannot prove is pure must not be evaluated
/// inside the web service, where a provider action would run in an http handler with no run to
/// record it, no retry, no timeout, and no cancellation.
///
/// note what is **not** here: a purity analysis of the parsed expression. `validate_fragment`
/// already refuses an effectful call in a pure fragment position, and the evaluator refuses it
/// again at run time. a third copy of that judgement in this crate would be one more thing to keep
/// in sync with the intrinsic catalog, and it would be the copy nobody remembered to update.
pub fn classify(source: &str, options: &CompileOptions) -> Result<Classification> {
    let trimmed = strip_comments(source);
    if trimmed.trim().is_empty() {
        return Err(ConsoleError::Empty);
    }

    if let Ok(lowered) =
        runinator_rexrap::validate_fragment(source, RexRapFragmentKind::Expression, options)
    {
        return Ok(Classification {
            kind: CellKind::Expression,
            lowered: Some(lowered),
            workflow_source: None,
        });
    }
    if let Ok(lowered) = runinator_rexrap::validate_fragment(source, RexRapFragmentKind::Do, options) {
        return Ok(Classification {
            kind: CellKind::Do,
            lowered: Some(lowered),
            workflow_source: None,
        });
    }

    Ok(Classification {
        kind: CellKind::Workflow,
        lowered: None,
        workflow_source: Some(source.to_string()),
    })
}

/// wrap a cell's statements in a workflow so they can be compiled.
///
/// a cell that already declares its own `workflow` block is passed through untouched: an author who
/// wrote one meant it, and wrapping it again would nest a workflow inside a workflow.
pub fn workflow_source(cell_source: &str, workflow_name: &str) -> String {
    if declares_workflow(cell_source) {
        return cell_source.to_string();
    }
    let body = cell_source
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("workflow \"{workflow_name}\" v1 {{\n{body}\n}}\n")
}

// true when the source already opens with a `workflow "..."` declaration.
fn declares_workflow(source: &str) -> bool {
    strip_comments(source).trim_start().starts_with("workflow")
}

// drop `//` line comments so a cell of only comments reads as empty and a commented-out `workflow`
// keyword does not look like a declaration.
fn strip_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
