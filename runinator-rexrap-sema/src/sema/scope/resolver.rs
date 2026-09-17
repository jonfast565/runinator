#[allow(unused_imports)]
use super::*;

pub(super) struct Resolver<'a> {
    pub(super) symbols: &'a Symbols,
}

impl Resolver<'_> {
    pub(super) fn resolve_block(
        &self,
        block: &Block,
        scope: &mut Vec<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        for stmt in block {
            self.resolve_stmt(stmt, scope, diagnostics);
        }
    }

    pub(super) fn resolve_stmt(
        &self,
        stmt: &Stmt,
        scope: &mut Vec<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let span = stmt.span;
        self.resolve_transitions(&stmt.transitions, span, diagnostics);

        let ctx = ExprCtx::Declarative;
        match &stmt.kind {
            StmtKind::Return(Some(value)) => self.resolve_expr(value, scope, ctx, diagnostics),
            StmtKind::Return(None) | StmtKind::Detach(_) => {}
            StmtKind::Action(action) => {
                self.resolve_reentry(&action.modifiers, span, diagnostics);
                for (_, value) in &action.args {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::TaskCall(call) => {
                self.resolve_reentry(&call.modifiers, span, diagnostics);
                for (_, value) in &call.args {
                    self.resolve_expr(value, scope, ctx, diagnostics);
                }
            }
            StmtKind::Compute(compute) => {
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
                    self.resolve_block(&branch.body, scope, diagnostics);
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

    pub(super) fn resolve_transitions(
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

    pub(super) fn resolve_reentry(
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

    pub(super) fn resolve_target(
        &self,
        target: &Target,
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
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
    pub(super) fn resolve_do(
        &self,
        compute: &runinator_rexrap_syntax::ast::ComputeStmt,
        scope: &mut Vec<String>,
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let effectful = crate::purity::block_is_effectful(&compute.body, &self.symbols.registry);
        let base = scope.len();
        self.resolve_compute_block(
            &compute.body,
            scope,
            span,
            diagnostics,
            BlockCtx::ComputeNode { effectful },
        );
        scope.truncate(base);
    }

    pub(super) fn resolve_compute_block(
        &self,
        body: &[runinator_rexrap_syntax::ast::ComputeLine],
        scope: &mut Vec<String>,
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
        ctx: BlockCtx,
    ) {
        use runinator_rexrap_syntax::ast::ComputeLine;
        // locals introduced at this block level, for duplicate detection.
        let block_start = scope.len();
        for line in body {
            match line {
                ComputeLine::Let { name, value, .. } => {
                    self.resolve_expr(value, scope, ExprCtx::Compute, diagnostics);
                    if scope[block_start..].iter().any(|n| n == name) {
                        diagnostics.push(Diagnostic::error(
                            value.span,
                            format!("compute local '{name}' is already defined"),
                        ));
                    }
                    scope.push(name.clone());
                }
                ComputeLine::Return(value) | ComputeLine::Expr(value) => {
                    self.resolve_expr(value, scope, ExprCtx::Compute, diagnostics);
                }
                ComputeLine::Goto(target) => match ctx {
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
                        if let runinator_rexrap_syntax::ast::Target::Label(label) = target
                            && !self.symbols.labels.contains(label)
                        {
                            diagnostics.push(Diagnostic::error(
                                span,
                                format!("compute goto references unknown label '{label}'"),
                            ));
                        }
                    }
                },
                ComputeLine::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    self.resolve_cond(cond, scope, ExprCtx::Compute, diagnostics);
                    let branch_start = scope.len();
                    self.resolve_compute_block(then_branch, scope, span, diagnostics, ctx);
                    scope.truncate(branch_start);
                    self.resolve_compute_block(else_branch, scope, span, diagnostics, ctx);
                    scope.truncate(branch_start);
                }
            }
        }
    }

    pub(super) fn resolve_cond(
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

    pub(super) fn resolve_expr(
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
            ExprKind::Str(literal) => {
                for part in &literal.parts {
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

impl Resolver<'_> {
    pub(super) fn resolve_path(
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
