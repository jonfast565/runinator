// name resolution and scope correctness. builds the global table of declared node ids
// (explicit `@id(...)` or `let` labels), then resolves every path head and transition
// target against it. loop/map variables live in a lexical scope stack mirroring the
// lowerer, so a variable referenced outside its body resolves to nothing and is reported.

use std::collections::HashSet;

use runinator_rexrap_syntax::ast::*;
use runinator_rexrap_syntax::errors::Span;

use super::{Diagnostic, child_blocks, effective_id};

/// reserved node ids the lowerer claims up front; user labels may not collide with them.
const RESERVED: [&str; 3] = ["start", "end", "fail"];

/// reserved path roots that always resolve regardless of declared labels.
const ROOTS: [&str; 6] = ["params", "prev", "run", "config", "secret", "interrupt"];

/// roots a workflow-parameter default may reference. defaults run at workflow start, before any
/// step, so `prev` and step outputs are not yet available; only start-time sources are allowed.
const DEFAULT_ROOTS: [&str; 4] = ["params", "config", "run", "secret"];

/// where an expression sits: a declarative position is evaluated eagerly by the reducer (so it may
/// only call pure intrinsics), while a compute position runs in `std.run`/`std.exec` and may call
/// effectful intrinsics. purity — not the grammar — decides which calls are legal where.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExprCtx {
    Declarative,
    Compute,
}

/// the declared-label table plus the callable registry, shared across this pass.

/// resolves references against one workflow's/function set's `Symbols`. `scope` (the lexical
/// loop/map/compute-local stack) is not folded in here: statement-level methods mutate it via an
/// explicit `&mut Vec<String>`, and expression-level methods read an extended, locally-scoped copy
/// (e.g. a lambda's params) that is never the same lifetime as a single owned field would allow.

/// resolve every function body's references against its parameters (functions are hermetic: only
/// their params, plus nested lambda params, are in scope). a body resolves in a compute context so
/// the purity pass — not name resolution — owns the effectful-call rule.
pub(super) fn resolve_function_bodies(
    functions: &[FunctionDef],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let symbols = Symbols {
        labels: HashSet::new(),
        registry: crate::registry::FunctionRegistry::build(functions),
    };
    let resolver = Resolver { symbols: &symbols };
    for def in functions {
        let mut scope: Vec<String> = def.params.iter().map(|param| param.name.clone()).collect();
        match &def.body {
            runinator_rexrap_syntax::ast::FnBody::Expr(expr) => {
                resolver.resolve_expr(expr, &scope, ExprCtx::Compute, diagnostics);
            }
            // a block body resolves like a compute block, with the params already in scope. the
            // `Function` context rejects any `goto` (a function body is not a graph region).
            runinator_rexrap_syntax::ast::FnBody::Block(lines) => {
                resolver.resolve_compute_block(
                    lines,
                    &mut scope,
                    def.span,
                    diagnostics,
                    BlockCtx::Function,
                );
            }
            // a `task fn` body is a statement region: resolve it like a workflow body, with the
            // parameters already bound.
            runinator_rexrap_syntax::ast::FnBody::Run(body) => {
                for stmt in body {
                    resolver.resolve_stmt(stmt, &mut scope, diagnostics);
                }
            }
        }
    }
}

/// collect declared labels (reporting duplicates), then resolve references and scopes.
pub(super) fn analyze(
    workflow: &Workflow,
    functions: &[FunctionDef],
    diagnostics: &mut Vec<Diagnostic>,
) -> Symbols {
    let mut labels = HashSet::new();
    collect_block(&workflow.body, &mut labels, diagnostics);
    // a `join <name> { … }` declares both its own name (the `continue` target) and whatever its
    // body binds, so both are in scope for the rest of the workflow.
    for join in &workflow.joins {
        if !labels.insert(join.name.clone()) {
            diagnostics.push(Diagnostic::error(
                join.span,
                format!("duplicate node id '{}'", join.name),
            ));
        }
        collect_block(&join.body, &mut labels, diagnostics);
    }
    let symbols = Symbols {
        labels,
        registry: crate::registry::FunctionRegistry::build(functions),
    };
    let resolver = Resolver { symbols: &symbols };

    // an explicit `start -> <target>` must name a declared step (or a terminal).
    if let Some(start) = &workflow.start {
        resolver.resolve_target(start, workflow.span, diagnostics);
    }

    // validate top-level workflow parameter defaults against the start-time roots.
    if let Some(TypeExpr::Struct { fields, .. }) = &workflow.input {
        for field in fields {
            if let Some(default) = &field.default {
                resolve_default_expr(default, &symbols.registry, diagnostics);
            }
        }
    }

    // a `trigger cron` schedule and a chained trigger target must be plain string literals.
    let require_literal = |value: &Expr, message: &str, diagnostics: &mut Vec<Diagnostic>| {
        let is_literal_string = matches!(
            &value.kind,
            ExprKind::Str(literal)
                if literal
                    .parts
                    .iter()
                    .all(|part| matches!(part, StrPart::Lit(_)))
        );
        if !is_literal_string {
            diagnostics.push(Diagnostic::error(value.span, message));
        }
    };
    for trigger in &workflow.triggers {
        match &trigger.kind {
            TriggerDeclKind::Cron {
                schedule,
                blackout_start,
                blackout_end,
                ..
            } => {
                require_literal(
                    schedule,
                    "trigger cron expression must be a string literal",
                    diagnostics,
                );
                for value in [blackout_start, blackout_end].into_iter().flatten() {
                    require_literal(
                        value,
                        "trigger blackout value must be a string literal",
                        diagnostics,
                    );
                }
            }
            TriggerDeclKind::Schedule {
                schedule,
                exclusions,
                ..
            } => {
                if !matches!(schedule.kind, ExprKind::Object(_)) {
                    diagnostics.push(Diagnostic::error(
                        schedule.span,
                        "trigger schedule must be an object literal",
                    ));
                }
                for exclusion in exclusions {
                    if !matches!(exclusion.kind, ExprKind::Object(_)) {
                        diagnostics.push(Diagnostic::error(
                            exclusion.span,
                            "blackout schedule must be an object literal",
                        ));
                    }
                }
            }
            TriggerDeclKind::Chained { target, .. } => {
                require_literal(
                    target,
                    "chained trigger target must be a string literal",
                    diagnostics,
                );
            }
        }
    }

    let mut scope = Vec::new();
    resolver.resolve_block(&workflow.body, &mut scope, diagnostics);
    // a join region resolves like the main flow; it is a continuation of it, not a separate scope.
    for join in &workflow.joins {
        resolver.resolve_block(&join.body, &mut scope, diagnostics);
    }
    symbols
}

fn collect_block(block: &Block, labels: &mut HashSet<String>, diagnostics: &mut Vec<Diagnostic>) {
    for stmt in block {
        if let Some(id) = effective_id(stmt) {
            if RESERVED.contains(&id) {
                diagnostics.push(Diagnostic::error(
                    stmt.span,
                    format!("node id '{id}' is reserved"),
                ));
            } else if !labels.insert(id.to_string()) {
                diagnostics.push(Diagnostic::error(
                    stmt.span,
                    format!("duplicate node id '{id}'"),
                ));
            }
        }
        for child in child_blocks(&stmt.kind) {
            collect_block(child, labels, diagnostics);
        }
    }
}

/// where a compute-line block sits: a `do` graph node (where `goto` jumps to another node, and
/// is forbidden in an effectful block that dispatches to a worker) or a function body (not a graph
/// region, so `goto` is always rejected).
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockCtx {
    ComputeNode { effectful: bool },
    Function,
}

/// validate a workflow-parameter default expression: only `DEFAULT_ROOTS` may head a reference.
fn resolve_default_expr(
    expr: &Expr,
    registry: &crate::registry::FunctionRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &expr.kind {
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::FileInclude { .. }
        | ExprKind::DirInclude { .. }
        | ExprKind::InlineCode { .. } => {}
        ExprKind::Str(literal) => {
            for part in &literal.parts {
                if let StrPart::Expr(inner) = part {
                    resolve_default_expr(inner, registry, diagnostics);
                }
            }
        }
        ExprKind::Path(segs) => {
            let Some(PathSeg::Key(head)) = segs.first() else {
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    "reference must start with an identifier",
                ));
                return;
            };
            if !DEFAULT_ROOTS.contains(&head.as_str()) {
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!(
                        "parameter default may only reference params, config, run, or secret, not '{head}'"
                    ),
                ));
            }
        }
        ExprKind::Array(items) => {
            for item in items {
                resolve_default_expr(item, registry, diagnostics);
            }
        }
        ExprKind::Object(entries) => {
            for (_, value) in entries {
                resolve_default_expr(value, registry, diagnostics);
            }
        }
        ExprKind::Concat(parts) | ExprKind::Coalesce(parts) => {
            for part in parts {
                resolve_default_expr(part, registry, diagnostics);
            }
        }
        ExprKind::Cast { expr, .. } => resolve_default_expr(expr, registry, diagnostics),
        ExprKind::Apply { callee, args } => {
            resolve_default_expr(callee, registry, diagnostics);
            for arg in args {
                resolve_default_expr(arg, registry, diagnostics);
            }
        }
        ExprKind::ToString(inner) | ExprKind::ToJson(inner) | ExprKind::Neg(inner) => {
            resolve_default_expr(inner, registry, diagnostics);
        }
        ExprKind::Compare { left, right, .. } => {
            resolve_default_expr(left, registry, diagnostics);
            resolve_default_expr(right, registry, diagnostics);
        }
        ExprKind::Ternary { cond, then, els } => {
            resolve_default_expr(cond, registry, diagnostics);
            resolve_default_expr(then, registry, diagnostics);
            resolve_default_expr(els, registry, diagnostics);
        }
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts {
                resolve_default_expr(part, registry, diagnostics);
            }
        }
        ExprKind::Call {
            name, args, named, ..
        } => {
            // defaults are evaluated eagerly at workflow start, so an effectful call (intrinsic or
            // user function) is not allowed.
            if registry.is_effectful(name) {
                let kind = if registry.is_user(name) {
                    "function"
                } else {
                    "intrinsic"
                };
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!(
                        "effectful {kind} '{name}' is not allowed in a workflow parameter default"
                    ),
                ));
            }
            for arg in args.iter().chain(named.iter().map(|(_, value)| value)) {
                resolve_default_expr(arg, registry, diagnostics);
            }
        }
        // a lambda is a compute-only form; the default grammar (`= expr`) never produces one.
        ExprKind::Lambda { .. } => diagnostics.push(Diagnostic::error(
            expr.span,
            "a lambda is not allowed in a workflow parameter default",
        )),
        ExprKind::Spread(_) => {}
    }
}

mod symbols;
pub(super) use symbols::Symbols;

mod resolver;
use resolver::Resolver;
