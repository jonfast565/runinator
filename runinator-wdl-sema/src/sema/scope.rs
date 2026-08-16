// name resolution and scope correctness. builds the global table of declared node ids
// (explicit `@id(...)` or `let` labels), then resolves every path head and transition
// target against it. loop/map variables live in a lexical scope stack mirroring the
// lowerer, so a variable referenced outside its body resolves to nothing and is reported.

use std::collections::HashSet;

use runinator_wdl_syntax::ast::*;
use runinator_wdl_syntax::errors::Span;

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
pub(super) struct Symbols {
    pub labels: HashSet<String>,
    pub registry: crate::registry::FunctionRegistry,
}

/// resolves references against one workflow's/function set's `Symbols`. `scope` (the lexical
/// loop/map/compute-local stack) is not folded in here: statement-level methods mutate it via an
/// explicit `&mut Vec<String>`, and expression-level methods read an extended, locally-scoped copy
/// (e.g. a lambda's params) that is never the same lifetime as a single owned field would allow.
struct Resolver<'a> {
    symbols: &'a Symbols,
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
    let resolver = Resolver { symbols: &symbols };
    for def in functions {
        let mut scope: Vec<String> = def.params.iter().map(|param| param.name.clone()).collect();
        match &def.body {
            runinator_wdl_syntax::ast::FnBody::Expr(expr) => {
                resolver.resolve_expr(expr, &scope, ExprCtx::Compute, diagnostics);
            }
            // a block body resolves like a compute block, with the params already in scope. the
            // `Function` context rejects any `goto` (a function body is not a graph region).
            runinator_wdl_syntax::ast::FnBody::Block(lines) => {
                resolver.resolve_do_block(
                    lines,
                    &mut scope,
                    def.span,
                    diagnostics,
                    BlockCtx::Function,
                );
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
            ExprKind::Str(parts) if parts.iter().all(|part| matches!(part, StrPart::Lit(_)))
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

impl Resolver<'_> {
    fn resolve_block(
        &self,
        block: &Block,
        scope: &mut Vec<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for stmt in block {
            self.resolve_stmt(stmt, scope, diagnostics);
        }
    }

    fn resolve_stmt(
        &self,
        stmt: &Stmt,
        scope: &mut Vec<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let span = stmt.span;
        self.resolve_transitions(&stmt.transitions, span, diagnostics);

        let ctx = ExprCtx::Declarative;
        match &stmt.kind {
            StmtKind::Action(action) => {
                self.resolve_reentry(&action.modifiers, span, diagnostics);
                for (_, value) in &action.args {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Do(compute) => {
                self.resolve_do(compute, scope, span, diagnostics);
            }
            StmtKind::Subflow(subflow) => {
                if let Some(run_name) = &subflow.run_name {
                    self.resolve_expr(run_name, scope, ctx, diagnostics);
                }
                for (_, value) in &subflow.params {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Wait(_) => {}
            StmtKind::Output(output) => {
                if let Some(data) = &output.data {
                    self.resolve_expr(data, scope, ctx, diagnostics);
                }
                for (_, source) in &output.items {
                    self.resolve_expr(source, scope, ctx, diagnostics);
                }
            }
            StmtKind::Yield(value) => self.resolve_expr(value, scope, ctx, diagnostics),
            StmtKind::Input(input) => {
                if let Some(prompt) = &input.prompt {
                    self.resolve_expr(prompt, scope, ctx, diagnostics);
                }
            }
            StmtKind::Approval(approval) => {
                self.resolve_expr(&approval.prompt, scope, ctx, diagnostics);
                for (_, value) in &approval.metadata {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Gate(gate) => {
                if let Some(when) = &gate.when {
                    self.resolve_cond(when, scope, ctx, diagnostics);
                }
                for (_, value) in &gate.metadata {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Signal(signal) => {
                for (_, value) in &signal.metadata {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Config(config) => {
                if let Some(name) = &config.name {
                    self.resolve_expr(name, scope, ctx, diagnostics);
                }
                if let Some(metadata) = &config.metadata {
                    self.resolve_expr(metadata, scope, ctx, diagnostics);
                }
            }
            StmtKind::Fail(message) => {
                if let Some(message) = message {
                    self.resolve_expr(message, scope, ctx, diagnostics);
                }
            }
            StmtKind::If(if_stmt) => {
                for (cond, body) in &if_stmt.arms {
                    self.resolve_cond(cond, scope, ctx, diagnostics);
                    self.resolve_block(body, scope, diagnostics);
                }
                if let Some(else_block) = &if_stmt.else_block {
                    self.resolve_block(else_block, scope, diagnostics);
                }
            }
            StmtKind::For(for_stmt) => {
                self.resolve_expr(&for_stmt.items, scope, ctx, diagnostics);
                // the cap is evaluated before iterating, so it cannot see the loop var.
                if let Some(limit) = &for_stmt.limit {
                    self.resolve_expr(limit, scope, ctx, diagnostics);
                }
                scope.push(for_stmt.var.clone());
                if let Some(index_var) = &for_stmt.index_var {
                    scope.push(index_var.clone());
                }
                self.resolve_block(&for_stmt.body, scope, diagnostics);
                if for_stmt.index_var.is_some() {
                    scope.pop();
                }
                scope.pop();
            }
            StmtKind::While(while_stmt) => {
                self.resolve_cond(&while_stmt.cond, scope, ctx, diagnostics);
                self.resolve_block(&while_stmt.body, scope, diagnostics);
            }
            StmtKind::Map(map_stmt) => {
                self.resolve_expr(&map_stmt.items, scope, ctx, diagnostics);
                scope.push(map_stmt.var.clone());
                self.resolve_block(&map_stmt.body, scope, diagnostics);
                scope.pop();
            }
            StmtKind::Match(match_stmt) => {
                self.resolve_expr(&match_stmt.subject, scope, ctx, diagnostics);
                for arm in &match_stmt.arms {
                    if let Some(equals) = &arm.equals {
                        self.resolve_expr(equals, scope, ctx, diagnostics);
                    }
                    if let Some(when) = &arm.when {
                        self.resolve_cond(when, scope, ctx, diagnostics);
                    }
                    self.resolve_block(&arm.body, scope, diagnostics);
                }
                if let Some(default) = &match_stmt.default {
                    self.resolve_block(default, scope, diagnostics);
                }
            }
            StmtKind::Parallel(parallel) => {
                for branch in &parallel.branches {
                    self.resolve_block(branch, scope, diagnostics);
                }
            }
            StmtKind::Race(race) => {
                for branch in &race.branches {
                    self.resolve_block(branch, scope, diagnostics);
                }
            }
            // `resume` carries no expressions, names, or bindings, so every pass is a no-op.
            StmtKind::Resume(_) => {}
            StmtKind::Try(try_stmt) => {
                self.resolve_block(&try_stmt.body, scope, diagnostics);
                if let Some(catch) = &try_stmt.catch {
                    self.resolve_block(catch, scope, diagnostics);
                }
                if let Some(finally) = &try_stmt.finally {
                    self.resolve_block(finally, scope, diagnostics);
                }
            }
            StmtKind::Assert(assert) => {
                for (_, cond) in &assert.assertions {
                    self.resolve_cond(cond, scope, ctx, diagnostics);
                }
            }
            StmtKind::Transform(transform) => {
                for (_, value) in &transform.bindings {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Audit(audit) => {
                self.resolve_expr(&audit.action, scope, ctx, diagnostics);
                for value in [
                    audit.actor.as_ref(),
                    audit.target.as_ref(),
                    audit.reason.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Await(await_stmt) => {
                if let Some(key) = &await_stmt.key {
                    self.resolve_expr(key, scope, ctx, diagnostics);
                }
            }
            StmtKind::Debounce(debounce) => {
                if let Some(key) = &debounce.key {
                    self.resolve_expr(key, scope, ctx, diagnostics);
                }
            }
            StmtKind::EventSource(es) => {
                if let Some(filter) = &es.filter {
                    self.resolve_cond(filter, scope, ctx, diagnostics);
                }
            }
            StmtKind::Mutex(mutex) => self.resolve_block(&mutex.body, scope, diagnostics),
            // these declare no references to resolve.
            StmtKind::Checkpoint(_)
            | StmtKind::Throttle(_)
            | StmtKind::Cooldown(_)
            | StmtKind::Collect(_)
            | StmtKind::Barrier(_)
            | StmtKind::CircuitBreaker(_) => {}
        }
    }

    fn resolve_transitions(
        &self,
        transitions: &TransitionClause,
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
            self.resolve_target(target, span, diagnostics);
        }
    }

    fn resolve_reentry(
        &self,
        modifiers: &Modifiers,
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if let Some(reentry) = &modifiers.reentry
            && let Some(target) = &reentry.on_exhausted
        {
            self.resolve_target(target, span, diagnostics);
        }
    }

    fn resolve_target(&self, target: &Target, span: Span, diagnostics: &mut Vec<Diagnostic>) {
        if let Target::Label(name) = target
            && !self.symbols.labels.contains(name)
        {
            diagnostics.push(Diagnostic::error(
                span,
                format!("transition targets unknown step '{name}'"),
            ));
        }
    }

    /// resolve a `do { }` block: thread block-scoped locals through `let`, reject duplicate
    /// locals, and enforce the purity rule that an effectful (`exec`) block may not use `goto`.
    fn resolve_do(
        &self,
        compute: &runinator_wdl_syntax::ast::DoStmt,
        scope: &mut Vec<String>,
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let effectful = crate::purity::block_is_effectful(&compute.body, &self.symbols.registry);
        let base = scope.len();
        self.resolve_do_block(
            &compute.body,
            scope,
            span,
            diagnostics,
            BlockCtx::ComputeNode { effectful },
        );
        scope.truncate(base);
    }

    fn resolve_do_block(
        &self,
        body: &[runinator_wdl_syntax::ast::DoLine],
        scope: &mut Vec<String>,
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
        ctx: BlockCtx,
    ) {
        use runinator_wdl_syntax::ast::DoLine;
        // locals introduced at this block level, for duplicate detection.
        let block_start = scope.len();
        for line in body {
            match line {
                DoLine::Let { name, value, .. } => {
                    self.resolve_expr(value, scope, ExprCtx::Compute, diagnostics);
                    if scope[block_start..].iter().any(|n| n == name) {
                        diagnostics.push(Diagnostic::error(
                            value.span,
                            format!("compute local '{name}' is already defined"),
                        ));
                    }
                    scope.push(name.clone());
                }
                DoLine::Return(value) | DoLine::Expr(value) => {
                    self.resolve_expr(value, scope, ExprCtx::Compute, diagnostics);
                }
                DoLine::Goto(target) => match ctx {
                    BlockCtx::Function => diagnostics.push(Diagnostic::error(
                        span,
                        "goto is not allowed in a function body (it is not a graph region)",
                    )),
                    BlockCtx::ComputeNode { effectful } => {
                        if effectful {
                            diagnostics.push(Diagnostic::error(
                                span,
                                "goto is not allowed in an effectful compute block (it dispatches to a worker)",
                            ));
                        }
                        if let runinator_wdl_syntax::ast::Target::Label(label) = target
                            && !self.symbols.labels.contains(label)
                        {
                            diagnostics.push(Diagnostic::error(
                                span,
                                format!("compute goto references unknown label '{label}'"),
                            ));
                        }
                    }
                },
                DoLine::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    self.resolve_cond(cond, scope, ExprCtx::Compute, diagnostics);
                    let branch_start = scope.len();
                    self.resolve_do_block(then_branch, scope, span, diagnostics, ctx);
                    scope.truncate(branch_start);
                    self.resolve_do_block(else_branch, scope, span, diagnostics, ctx);
                    scope.truncate(branch_start);
                }
            }
        }
    }

    fn resolve_cond(
        &self,
        cond: &Cond,
        scope: &[String],
        ctx: ExprCtx,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        match &cond.kind {
            CondKind::All(parts) | CondKind::Any(parts) => {
                for part in parts {
                    self.resolve_cond(part, scope, ctx, diagnostics);
                }
            }
            CondKind::Not(inner) => self.resolve_cond(inner, scope, ctx, diagnostics),
            CondKind::Expr(expr) => self.resolve_expr(expr, scope, ctx, diagnostics),
            CondKind::Cmp { left, right, .. } => {
                self.resolve_expr(left, scope, ctx, diagnostics);
                self.resolve_expr(right, scope, ctx, diagnostics);
            }
            CondKind::Exists(expr) => self.resolve_expr(expr, scope, ctx, diagnostics),
        }
    }

    fn resolve_expr(
        &self,
        expr: &Expr,
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
                        self.resolve_expr(inner, scope, ctx, diagnostics);
                    }
                }
            }
            ExprKind::Path(segs) => self.resolve_path(segs, scope, expr.span, diagnostics),
            ExprKind::Array(items) => {
                for item in items {
                    self.resolve_expr(item, scope, ctx, diagnostics);
                }
            }
            ExprKind::Object(entries) => {
                for (_, value) in entries {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            ExprKind::Concat(parts) | ExprKind::Coalesce(parts) => {
                for part in parts {
                    self.resolve_expr(part, scope, ctx, diagnostics);
                }
            }
            ExprKind::Cast { expr, .. } => self.resolve_expr(expr, scope, ctx, diagnostics),
            ExprKind::Apply { callee, args } => {
                self.resolve_expr(callee, scope, ctx, diagnostics);
                for arg in args {
                    self.resolve_expr(arg, scope, ctx, diagnostics);
                }
            }
            ExprKind::ToString(inner) | ExprKind::ToJson(inner) | ExprKind::Neg(inner) => {
                self.resolve_expr(inner, scope, ctx, diagnostics);
            }
            ExprKind::Compare { left, right, .. } => {
                self.resolve_expr(left, scope, ctx, diagnostics);
                self.resolve_expr(right, scope, ctx, diagnostics);
            }
            ExprKind::Ternary { cond, then, els } => {
                self.resolve_expr(cond, scope, ctx, diagnostics);
                self.resolve_expr(then, scope, ctx, diagnostics);
                self.resolve_expr(els, scope, ctx, diagnostics);
            }
            ExprKind::Add(parts)
            | ExprKind::Sub(parts)
            | ExprKind::Mul(parts)
            | ExprKind::Div(parts)
            | ExprKind::Mod(parts) => {
                for part in parts {
                    self.resolve_expr(part, scope, ctx, diagnostics);
                }
            }
            ExprKind::Call {
                name, args, named, ..
            } => {
                let is_user = self.symbols.registry.is_user(name);
                // a local bound to a first-class lambda is a valid callee; its type-correctness
                // (that it really is a function, and its arity) is checked by the type pass.
                let is_local = scope.iter().any(|local| local == name);
                // validate the call against the callable vocabulary: unknown names (typos), arity,
                // and keyword-argument mistakes are reported here rather than failing late at the
                // worker.
                if !self.symbols.registry.knows(name) && !is_local {
                    diagnostics.push(Diagnostic::error(
                        expr.span,
                        format!("unknown function '{name}'"),
                    ));
                } else if !is_user
                    && !is_local
                    && let Some((min, max)) = runinator_compute::intrinsic_arity(name)
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
                } else if ctx == ExprCtx::Declarative && self.symbols.registry.is_effectful(name) {
                    // a declarative position is folded eagerly in the reducer, which cannot run
                    // side effects; an effectful call (intrinsic or user function) must live in a
                    // `do` block (it dispatches to a worker).
                    let kind = if is_user { "function" } else { "intrinsic" };
                    diagnostics.push(Diagnostic::error(
                        expr.span,
                        format!("effectful {kind} '{name}' must be inside a compute block"),
                    ));
                } else if let Err(err) = self.symbols.registry.resolve_args(name, args, named) {
                    // keyword/arity resolution errors (unknown keyword, missing required, gaps).
                    diagnostics.push(Diagnostic::error(expr.span, err));
                }
                for arg in args.iter().chain(named.iter().map(|(_, value)| value)) {
                    self.resolve_expr(arg, scope, ctx, diagnostics);
                }
            }
            // a lambda introduces its params as references available only inside its body.
            ExprKind::Lambda { params, body } => {
                let mut inner = scope.to_vec();
                inner.extend(params.iter().cloned());
                self.resolve_expr(body, &inner, ctx, diagnostics);
            }
            // spreads are expanded before sema runs; nothing to resolve.
            ExprKind::Spread(_) => {}
        }
    }
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
        ExprKind::Str(parts) => {
            for part in parts {
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

impl Resolver<'_> {
    fn resolve_path(
        &self,
        segs: &[PathSeg],
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
            || self.symbols.labels.contains(head);
        if !resolved {
            diagnostics.push(Diagnostic::error(
                span,
                format!("unknown reference '{head}'"),
            ));
        }
    }
}
