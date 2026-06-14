// name resolution and scope correctness. builds the global table of declared node ids
// (explicit `@id(...)` or `let` labels), then resolves every path head and transition
// target against it. loop/map variables live in a lexical scope stack mirroring the
// lowerer, so a variable referenced outside its body resolves to nothing and is reported.

use std::collections::HashSet;

use crate::ast::*;
use crate::errors::Span;

use super::{Diagnostic, child_blocks, effective_id};

/// reserved node ids the lowerer claims up front; user labels may not collide with them.
const RESERVED: [&str; 3] = ["start", "end", "fail"];

/// reserved path roots that always resolve regardless of declared labels.
const ROOTS: [&str; 5] = ["params", "prev", "run", "config", "secret"];

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
pub(super) struct Symbols {
    pub labels: HashSet<String>,
    pub registry: crate::registry::FunctionRegistry,
}

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
    for def in functions {
        let scope: Vec<String> = def.params.iter().map(|param| param.name.clone()).collect();
        resolve_expr(&def.body, &symbols, &scope, ExprCtx::Compute, diagnostics);
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
    let symbols = Symbols {
        labels,
        registry: crate::registry::FunctionRegistry::build(functions),
    };

    // an explicit `start -> <target>` must name a declared step (or a terminal).
    if let Some(start) = &workflow.start {
        resolve_target(start, &symbols, workflow.span, diagnostics);
    }

    // validate top-level workflow parameter defaults against the start-time roots.
    if let Some(TypeExpr::Struct { fields, .. }) = &workflow.input {
        for field in fields {
            if let Some(default) = &field.default {
                resolve_default_expr(default, diagnostics);
            }
        }
    }

    // a `trigger cron` schedule must be a plain string literal (the cron expression).
    for trigger in &workflow.triggers {
        let is_literal_string = matches!(
            &trigger.schedule.kind,
            ExprKind::Str(parts) if parts.iter().all(|part| matches!(part, StrPart::Lit(_)))
        );
        if !is_literal_string {
            diagnostics.push(Diagnostic::error(
                trigger.schedule.span,
                "trigger cron expression must be a string literal",
            ));
        }
        for value in [&trigger.blackout_start, &trigger.blackout_end]
            .into_iter()
            .flatten()
        {
            let is_literal_string = matches!(
                &value.kind,
                ExprKind::Str(parts) if parts.iter().all(|part| matches!(part, StrPart::Lit(_)))
            );
            if !is_literal_string {
                diagnostics.push(Diagnostic::error(
                    value.span,
                    "trigger blackout value must be a string literal",
                ));
            }
        }
    }

    let mut scope = Vec::new();
    resolve_block(&workflow.body, &symbols, &mut scope, diagnostics);
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

fn resolve_block(
    block: &Block,
    symbols: &Symbols,
    scope: &mut Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for stmt in block {
        resolve_stmt(stmt, symbols, scope, diagnostics);
    }
}

fn resolve_stmt(
    stmt: &Stmt,
    symbols: &Symbols,
    scope: &mut Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let span = stmt.span;
    resolve_transitions(&stmt.transitions, symbols, span, diagnostics);

    let ctx = ExprCtx::Declarative;
    match &stmt.kind {
        StmtKind::Action(action) => {
            resolve_reentry(&action.modifiers, symbols, span, diagnostics);
            for (_, value) in &action.args {
                resolve_expr(value, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Compute(compute) => {
            resolve_compute(compute, symbols, scope, span, diagnostics);
        }
        StmtKind::Subflow(subflow) => {
            if let Some(run_name) = &subflow.run_name {
                resolve_expr(run_name, symbols, scope, ctx, diagnostics);
            }
            for (_, value) in &subflow.params {
                resolve_expr(value, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Wait(_) => {}
        StmtKind::Output(output) => {
            if let Some(data) = &output.data {
                resolve_expr(data, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Input(input) => {
            if let Some(prompt) = &input.prompt {
                resolve_expr(prompt, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Approval(approval) => {
            resolve_expr(&approval.prompt, symbols, scope, ctx, diagnostics);
            for (_, value) in &approval.metadata {
                resolve_expr(value, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Gate(gate) => {
            if let Some(when) = &gate.when {
                resolve_cond(when, symbols, scope, ctx, diagnostics);
            }
            for (_, value) in &gate.metadata {
                resolve_expr(value, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Signal(signal) => {
            for (_, value) in &signal.metadata {
                resolve_expr(value, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Config(config) => {
            if let Some(name) = &config.name {
                resolve_expr(name, symbols, scope, ctx, diagnostics);
            }
            if let Some(metadata) = &config.metadata {
                resolve_expr(metadata, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::Fail(message) => {
            if let Some(message) = message {
                resolve_expr(message, symbols, scope, ctx, diagnostics);
            }
        }
        StmtKind::If(if_stmt) => {
            for (cond, body) in &if_stmt.arms {
                resolve_cond(cond, symbols, scope, ctx, diagnostics);
                resolve_block(body, symbols, scope, diagnostics);
            }
            if let Some(else_block) = &if_stmt.else_block {
                resolve_block(else_block, symbols, scope, diagnostics);
            }
        }
        StmtKind::For(for_stmt) => {
            resolve_expr(&for_stmt.items, symbols, scope, ctx, diagnostics);
            scope.push(for_stmt.var.clone());
            resolve_block(&for_stmt.body, symbols, scope, diagnostics);
            scope.pop();
        }
        StmtKind::While(while_stmt) => {
            resolve_cond(&while_stmt.cond, symbols, scope, ctx, diagnostics);
            resolve_block(&while_stmt.body, symbols, scope, diagnostics);
        }
        StmtKind::Map(map_stmt) => {
            resolve_expr(&map_stmt.items, symbols, scope, ctx, diagnostics);
            scope.push(map_stmt.var.clone());
            resolve_block(&map_stmt.body, symbols, scope, diagnostics);
            scope.pop();
        }
        StmtKind::Match(match_stmt) => {
            resolve_expr(&match_stmt.subject, symbols, scope, ctx, diagnostics);
            for arm in &match_stmt.arms {
                if let Some(equals) = &arm.equals {
                    resolve_expr(equals, symbols, scope, ctx, diagnostics);
                }
                if let Some(when) = &arm.when {
                    resolve_cond(when, symbols, scope, ctx, diagnostics);
                }
                resolve_block(&arm.body, symbols, scope, diagnostics);
            }
            if let Some(default) = &match_stmt.default {
                resolve_block(default, symbols, scope, diagnostics);
            }
        }
        StmtKind::Parallel(parallel) => {
            for branch in &parallel.branches {
                resolve_block(branch, symbols, scope, diagnostics);
            }
        }
        StmtKind::Race(race) => {
            for branch in &race.branches {
                resolve_block(branch, symbols, scope, diagnostics);
            }
        }
        StmtKind::Try(try_stmt) => {
            resolve_block(&try_stmt.body, symbols, scope, diagnostics);
            if let Some(catch) = &try_stmt.catch {
                resolve_block(catch, symbols, scope, diagnostics);
            }
            if let Some(finally) = &try_stmt.finally {
                resolve_block(finally, symbols, scope, diagnostics);
            }
        }
    }
}

fn resolve_transitions(
    transitions: &TransitionClause,
    symbols: &Symbols,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for target in [
        &transitions.next,
        &transitions.on_success,
        &transitions.on_failure,
        &transitions.on_timeout,
        &transitions.on_reject,
    ]
    .into_iter()
    .flatten()
    {
        resolve_target(target, symbols, span, diagnostics);
    }
}

fn resolve_reentry(
    modifiers: &Modifiers,
    symbols: &Symbols,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(reentry) = &modifiers.reentry {
        if let Some(target) = &reentry.on_exhausted {
            resolve_target(target, symbols, span, diagnostics);
        }
    }
}

fn resolve_target(
    target: &Target,
    symbols: &Symbols,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Target::Label(name) = target {
        if !symbols.labels.contains(name) {
            diagnostics.push(Diagnostic::error(
                span,
                format!("transition targets unknown step '{name}'"),
            ));
        }
    }
}

/// resolve a `compute { }` block: thread block-scoped locals through `let`, reject duplicate
/// locals, and enforce the purity rule that an effectful (`exec`) block may not use `goto`.
fn resolve_compute(
    compute: &crate::ast::ComputeStmt,
    symbols: &Symbols,
    scope: &mut Vec<String>,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let effectful = crate::purity::block_is_effectful(&compute.body, &symbols.registry);
    let base = scope.len();
    resolve_compute_block(&compute.body, symbols, scope, span, diagnostics, effectful);
    scope.truncate(base);
}

fn resolve_compute_block(
    body: &[crate::ast::ComputeLine],
    symbols: &Symbols,
    scope: &mut Vec<String>,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
    effectful: bool,
) {
    use crate::ast::ComputeLine;
    // locals introduced at this block level, for duplicate detection.
    let block_start = scope.len();
    for line in body {
        match line {
            ComputeLine::Let { name, value, .. } => {
                resolve_expr(value, symbols, scope, ExprCtx::Compute, diagnostics);
                if scope[block_start..].iter().any(|n| n == name) {
                    diagnostics.push(Diagnostic::error(
                        value.span,
                        format!("compute local '{name}' is already defined"),
                    ));
                }
                scope.push(name.clone());
            }
            ComputeLine::Return(value) | ComputeLine::Expr(value) => {
                resolve_expr(value, symbols, scope, ExprCtx::Compute, diagnostics);
            }
            ComputeLine::Goto(target) => {
                if effectful {
                    diagnostics.push(Diagnostic::error(
                        span,
                        "goto is not allowed in an effectful compute block (it dispatches to a worker)",
                    ));
                }
                if let crate::ast::Target::Label(label) = target
                    && !symbols.labels.contains(label)
                {
                    diagnostics.push(Diagnostic::error(
                        span,
                        format!("compute goto references unknown label '{label}'"),
                    ));
                }
            }
            ComputeLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                resolve_cond(cond, symbols, scope, ExprCtx::Compute, diagnostics);
                let branch_start = scope.len();
                resolve_compute_block(then_branch, symbols, scope, span, diagnostics, effectful);
                scope.truncate(branch_start);
                resolve_compute_block(else_branch, symbols, scope, span, diagnostics, effectful);
                scope.truncate(branch_start);
            }
        }
    }
}

fn resolve_cond(
    cond: &Cond,
    symbols: &Symbols,
    scope: &[String],
    ctx: ExprCtx,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => {
            for part in parts {
                resolve_cond(part, symbols, scope, ctx, diagnostics);
            }
        }
        CondKind::Not(inner) => resolve_cond(inner, symbols, scope, ctx, diagnostics),
        CondKind::Expr(expr) => resolve_expr(expr, symbols, scope, ctx, diagnostics),
        CondKind::Cmp { left, right, .. } => {
            resolve_expr(left, symbols, scope, ctx, diagnostics);
            resolve_expr(right, symbols, scope, ctx, diagnostics);
        }
        CondKind::Exists(expr) => resolve_expr(expr, symbols, scope, ctx, diagnostics),
    }
}

fn resolve_expr(
    expr: &Expr,
    symbols: &Symbols,
    scope: &[String],
    ctx: ExprCtx,
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
        ExprKind::Str(parts) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    resolve_expr(inner, symbols, scope, ctx, diagnostics);
                }
            }
        }
        ExprKind::Path(segs) => resolve_path(segs, symbols, scope, expr.span, diagnostics),
        ExprKind::Array(items) => {
            for item in items {
                resolve_expr(item, symbols, scope, ctx, diagnostics);
            }
        }
        ExprKind::Object(entries) => {
            for (_, value) in entries {
                resolve_expr(value, symbols, scope, ctx, diagnostics);
            }
        }
        ExprKind::Concat(parts) | ExprKind::Coalesce(parts) => {
            for part in parts {
                resolve_expr(part, symbols, scope, ctx, diagnostics);
            }
        }
        ExprKind::ToString(inner) | ExprKind::ToJson(inner) | ExprKind::Neg(inner) => {
            resolve_expr(inner, symbols, scope, ctx, diagnostics);
        }
        ExprKind::Compare { left, right, .. } => {
            resolve_expr(left, symbols, scope, ctx, diagnostics);
            resolve_expr(right, symbols, scope, ctx, diagnostics);
        }
        ExprKind::Ternary { cond, then, els } => {
            resolve_expr(cond, symbols, scope, ctx, diagnostics);
            resolve_expr(then, symbols, scope, ctx, diagnostics);
            resolve_expr(els, symbols, scope, ctx, diagnostics);
        }
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts {
                resolve_expr(part, symbols, scope, ctx, diagnostics);
            }
        }
        ExprKind::Call { name, args, named } => {
            let is_user = symbols.registry.is_user(name);
            // validate the call against the callable vocabulary: unknown names (typos), arity, and
            // keyword-argument mistakes are reported here rather than failing late at the worker.
            if !symbols.registry.knows(name) {
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!("unknown function '{name}'"),
                ));
            } else if !is_user
                && let Some((min, max)) = runinator_workflows::intrinsic_arity(name)
                && named.is_empty()
                && (args.len() < min || args.len() > max)
            {
                let expected = if min == max {
                    format!("{min}")
                } else {
                    format!("{min}-{max}")
                };
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!(
                        "intrinsic '{name}' expects {expected} argument(s), got {}",
                        args.len()
                    ),
                ));
            } else if ctx == ExprCtx::Declarative
                && runinator_workflows::EFFECTFUL_INTRINSIC_NAMES.contains(&name.as_str())
            {
                // a declarative position is folded eagerly in the reducer, which cannot run side
                // effects; an effectful call must live in a `compute` block (it dispatches to a worker).
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!("effectful intrinsic '{name}' must be inside a compute block"),
                ));
            } else if let Err(err) = symbols.registry.resolve_args(name, args, named) {
                // keyword/arity resolution errors (unknown keyword, missing required, gaps).
                diagnostics.push(Diagnostic::error(expr.span, err));
            }
            for arg in args.iter().chain(named.iter().map(|(_, value)| value)) {
                resolve_expr(arg, symbols, scope, ctx, diagnostics);
            }
        }
        // a lambda introduces its params as references available only inside its body.
        ExprKind::Lambda { params, body } => {
            let mut inner = scope.to_vec();
            inner.extend(params.iter().cloned());
            resolve_expr(body, symbols, &inner, ctx, diagnostics);
        }
        // spreads are expanded before sema runs; nothing to resolve.
        ExprKind::Spread(_) => {}
    }
}

/// validate a workflow-parameter default expression: only `DEFAULT_ROOTS` may head a reference.
fn resolve_default_expr(expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.kind {
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::FileInclude { .. }
        | ExprKind::DirInclude { .. }
        | ExprKind::InlineCode { .. } => {}
        ExprKind::Str(parts) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    resolve_default_expr(inner, diagnostics);
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
                resolve_default_expr(item, diagnostics);
            }
        }
        ExprKind::Object(entries) => {
            for (_, value) in entries {
                resolve_default_expr(value, diagnostics);
            }
        }
        ExprKind::Concat(parts) | ExprKind::Coalesce(parts) => {
            for part in parts {
                resolve_default_expr(part, diagnostics);
            }
        }
        ExprKind::ToString(inner) | ExprKind::ToJson(inner) | ExprKind::Neg(inner) => {
            resolve_default_expr(inner, diagnostics);
        }
        ExprKind::Compare { left, right, .. } => {
            resolve_default_expr(left, diagnostics);
            resolve_default_expr(right, diagnostics);
        }
        ExprKind::Ternary { cond, then, els } => {
            resolve_default_expr(cond, diagnostics);
            resolve_default_expr(then, diagnostics);
            resolve_default_expr(els, diagnostics);
        }
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts {
                resolve_default_expr(part, diagnostics);
            }
        }
        ExprKind::Call { name, args, named } => {
            // defaults are evaluated eagerly at workflow start, so an effectful call is not allowed.
            if runinator_workflows::EFFECTFUL_INTRINSIC_NAMES.contains(&name.as_str()) {
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!(
                        "effectful intrinsic '{name}' is not allowed in a workflow parameter default"
                    ),
                ));
            }
            for arg in args.iter().chain(named.iter().map(|(_, value)| value)) {
                resolve_default_expr(arg, diagnostics);
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

fn resolve_path(
    segs: &[PathSeg],
    symbols: &Symbols,
    scope: &[String],
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(PathSeg::Key(head)) = segs.first() else {
        diagnostics.push(Diagnostic::error(
            span,
            "reference must start with an identifier",
        ));
        return;
    };
    let resolved = ROOTS.contains(&head.as_str())
        || scope.iter().any(|name| name == head)
        || symbols.labels.contains(head);
    if !resolved {
        diagnostics.push(Diagnostic::error(
            span,
            format!("unknown reference '{head}'"),
        ));
    }
}
