//! deciding whether a cell can be answered in process or needs a run.

use runinator_models::{console::ConsoleFunction, value::Value};
use runinator_rexrap::{CompileOptions, RexRapFragmentKind};

use crate::errors::{ConsoleError, Result};

/// what a cell turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CellKind {
    /// a single expression, evaluable in process against the session's bindings.
    Expression,
    /// a `compute { ... }` program, still evaluable in process.
    Do,
    /// function declarations with no executable body. Successful execution publishes them to the
    /// session library and deliberately creates no value binding.
    Library,
    /// anything effectful or structural becomes a scratch workflow and goes through the reducer.
    Workflow,
}

/// a classified cell plus what the caller needs to act on it.
#[derive(Debug, Clone)]
pub struct Classification {
    pub kind: CellKind,
    /// the lowered fragment, for a pure cell. Kept for callers that inspect classification; the
    /// engine evaluates source again with its session function module.
    pub lowered: Option<Value>,
    /// the normalized source of a pure fragment (`compute { ... }` loses its statement keyword).
    pub pure_source: Option<String>,
    /// true when evaluation must use the compiled session-function module rather than the isolated
    /// fragment evaluator.
    pub uses_function_module: bool,
    /// the rexrap source to compile, for a workflow cell. `None` otherwise.
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
            CellKind::Library | CellKind::Workflow => None,
        }
    }
}

/// classify one cell with no session library. Kept as the simple public entry point for isolated
/// callers; the engine uses [`classify_with_functions`] for a real console session.
pub fn classify(source: &str, options: &CompileOptions) -> Result<Classification> {
    classify_with_functions(source, options, &[])
}

/// classify one cell in the context of the session's active function library.
///
/// The order is intentional: complete documents and console modules are structural forms, so they
/// must be recognised before trying them as fragments. Pure validation is performed through a tiny
/// ordinary workflow carrying the library declarations; that is what lets named pure functions stay
/// in-process while a `task fn` call is conservatively routed to the durable workflow path.
pub fn classify_with_functions(
    source: &str,
    options: &CompileOptions,
    functions: &[ConsoleFunction],
) -> Result<Classification> {
    let trimmed = strip_comments(source);
    if trimmed.trim().is_empty() {
        return Err(ConsoleError::Empty);
    }

    if let Ok(document) = runinator_rexrap::parse_document(source) {
        if document.workflows.len() != 1 {
            return Err(ConsoleError::Uncompilable(format!(
                "the console accepts exactly one workflow document, found {}",
                document.workflows.len()
            )));
        }
        return Ok(workflow(source));
    }

    if let Ok(module) = runinator_rexrap::parse_console_module(source) {
        return Ok(if module.run_block_span.is_some() {
            workflow(source)
        } else {
            Classification {
                kind: CellKind::Library,
                lowered: None,
                pure_source: None,
                uses_function_module: false,
                workflow_source: None,
            }
        });
    }

    let function_sources = functions
        .iter()
        .map(|function| function.source.clone())
        .collect::<Vec<_>>();
    let function_names = functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    for (kind, fragment) in pure_candidates(source) {
        // Isolated fragments retain their long-standing permissive validation for references such
        // as `cells.load.rows`: they do not need a document-level type environment. Any active
        // session function, though, needs the compiled module for correct named-argument binding
        // and for task-function purity checking.
        if let Ok(lowered) = runinator_rexrap::validate_fragment(&fragment, kind, options)
            && !calls_session_function(&lowered, &function_names)
        {
            return Ok(Classification {
                kind: match kind {
                    RexRapFragmentKind::Expression => CellKind::Expression,
                    RexRapFragmentKind::Do => CellKind::Do,
                    RexRapFragmentKind::Condition => {
                        unreachable!("conditions are not console cells")
                    }
                },
                lowered: Some(lowered),
                pure_source: Some(fragment),
                uses_function_module: false,
                workflow_source: None,
            });
        }
        if runinator_rexrap::validate_fragment_with_functions(
            &fragment,
            kind,
            &function_sources,
            options,
        )
        .is_ok()
        {
            return Ok(Classification {
                kind: match kind {
                    RexRapFragmentKind::Expression => CellKind::Expression,
                    RexRapFragmentKind::Do => CellKind::Do,
                    RexRapFragmentKind::Condition => {
                        unreachable!("conditions are not console cells")
                    }
                },
                lowered: Some(Value::Null),
                pure_source: Some(fragment),
                uses_function_module: true,
                workflow_source: None,
            });
        }
    }

    // The fallback remains unconditional. A cell this code cannot prove pure must be run through
    // the normal workflow lifecycle, never evaluated inside an HTTP request.
    Ok(workflow(source))
}

fn calls_session_function(value: &Value, function_names: &std::collections::HashSet<&str>) -> bool {
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| calls_session_function(value, function_names)),
        Value::Object(values) => {
            value
                .get("$call")
                .and_then(Value::as_str)
                .is_some_and(|name| function_names.contains(name))
                || values
                    .values()
                    .any(|value| calls_session_function(value, function_names))
        }
        _ => false,
    }
}

fn workflow(source: &str) -> Classification {
    Classification {
        kind: CellKind::Workflow,
        lowered: None,
        pure_source: None,
        uses_function_module: false,
        workflow_source: Some(source.to_string()),
    }
}

fn pure_candidates(source: &str) -> Vec<(RexRapFragmentKind, String)> {
    let mut candidates = vec![(RexRapFragmentKind::Expression, source.to_string())];
    let compute = source.trim_start();
    if let Some(body) = compute.strip_prefix("compute") {
        // The keyword form is a convenient REPL spelling. `parse_do_fragment` deliberately owns
        // just the pure `{ ... }` body because workflow `compute` statements live elsewhere.
        if body
            .chars()
            .next()
            .is_some_and(|character| character.is_whitespace() || character == '{')
        {
            candidates.push((RexRapFragmentKind::Do, body.trim_start().to_string()));
        }
    }
    candidates.push((RexRapFragmentKind::Do, source.to_string()));
    candidates
}

/// Wrap a cell's statements in a workflow so they can be compiled.
///
/// Retained for callers outside the session runtime. Use [`workflow_source_with_functions`] when
/// compiling a real console cell, so active declarations are brought into scope.
pub fn workflow_source(cell_source: &str, workflow_name: &str) -> String {
    workflow_source_with_functions(cell_source, workflow_name, &[])
}

/// Build the complete scratch document for a workflow cell, merging active session definitions
/// with local ones. Local declarations win for the cell being compiled; publication happens later,
/// only after the cell succeeds.
pub fn workflow_source_with_functions(
    cell_source: &str,
    workflow_name: &str,
    functions: &[ConsoleFunction],
) -> String {
    let local_names = runinator_rexrap::function_definitions(cell_source)
        .unwrap_or_default()
        .into_iter()
        .map(|function| function.name)
        .collect::<std::collections::HashSet<_>>();
    let active = functions
        .iter()
        .filter(|function| !local_names.contains(&function.name))
        .map(|function| function.source.as_str())
        .collect::<Vec<_>>();
    let declarations = active.join("\n\n");

    if runinator_rexrap::parse_document(cell_source).is_ok() {
        return insert_declarations(cell_source, &declarations);
    }

    if let Ok(module) = runinator_rexrap::parse_console_module(cell_source)
        && let Some(span) = module.run_block_span
    {
        let prefix = insert_declarations(&cell_source[..span.start], &declarations);
        let body = indent(&cell_source[span.start..span.end], 4);
        let suffix = &cell_source[span.end..];
        return format!("{prefix}\nworkflow \"{workflow_name}\" v1 {{\n{body}\n}}\n{suffix}");
    }

    let body = indent(cell_source, 8);
    let prefix = if declarations.is_empty() {
        String::new()
    } else {
        format!("{declarations}\n\n")
    };
    format!("{prefix}workflow \"{workflow_name}\" v1 {{\n    do {{\n{body}\n    }}\n}}\n")
}

fn insert_declarations(source: &str, declarations: &str) -> String {
    if declarations.is_empty() {
        return source.to_string();
    }
    let trimmed = source.trim_start();
    if trimmed.starts_with("language rexrap-1") {
        let start = source.len() - trimmed.len();
        let header_end = start + "language rexrap-1".len();
        return format!(
            "{}\n\n{}{}",
            &source[..header_end],
            declarations,
            &source[header_end..]
        );
    }
    format!("{declarations}\n\n{source}")
}

fn indent(source: &str, spaces: usize) -> String {
    let padding = " ".repeat(spaces);
    source
        .lines()
        .map(|line| format!("{padding}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// drop `//` line comments so a cell of only comments reads as empty and a commented-out
// declaration does not look executable.
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
