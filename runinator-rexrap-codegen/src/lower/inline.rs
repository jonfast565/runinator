//! inlining a `task fn` at a call site.
//!
//! a `task fn` is a named region of runtime statements, not a child run: calling it splices its
//! body into the caller's graph. binding the parameters is therefore *substitution*, not a runtime
//! frame — the argument expressions are written into the body before it is lowered, so a call site
//! costs exactly what writing the statements inline would have cost.
//!
//! this is also what keeps the language free of colored functions. the callee declares only that
//! its body does runtime work; nothing here decides whether the caller joins it inline or takes a
//! `task[T]`, because that is the call site's `async` marker to choose.

use std::collections::HashMap;

use runinator_rexrap_syntax::ast::*;
use runinator_rexrap_syntax::comments::CommentSet;
use runinator_rexrap_syntax::errors::RexRapError;

/// substitute `bindings` into a cloned body and namespace every label the body declares.
///
/// labels become graph node ids, so two calls to the same `task fn` must not collide; `prefix`
/// is the call site's own node id, which is unique by construction.
pub(super) fn inline_body(
    body: &[Stmt],
    bindings: &HashMap<String, Expr>,
    prefix: &str,
) -> Result<Vec<Stmt>, RexRapError> {
    let labels = declared_labels(body);
    let renames: HashMap<String, String> = labels
        .iter()
        .map(|label| (label.clone(), format!("{prefix}__{label}")))
        .collect();
    let mut out = body.to_vec();
    for stmt in out.iter_mut() {
        rewrite_stmt(stmt, bindings, &renames);
    }
    Ok(out)
}

/// every label bound anywhere in the region, including inside nested blocks.
fn declared_labels(body: &[Stmt]) -> Vec<String> {
    let mut out = Vec::new();
    for stmt in body {
        if let Some(label) = &stmt.label {
            out.push(label.clone());
        }
        for block in child_blocks(&stmt.kind) {
            out.extend(declared_labels(block));
        }
    }
    out
}

fn child_blocks(kind: &StmtKind) -> Vec<&[Stmt]> {
    match kind {
        StmtKind::If(stmt) => {
            let mut blocks: Vec<&[Stmt]> =
                stmt.arms.iter().map(|(_, body)| body.as_slice()).collect();
            blocks.extend(stmt.else_block.as_deref());
            blocks
        }
        StmtKind::For(stmt) => vec![&stmt.body],
        StmtKind::While(stmt) => vec![&stmt.body],
        StmtKind::Map(stmt) => vec![&stmt.body],
        StmtKind::Try(stmt) => {
            let mut blocks: Vec<&[Stmt]> = vec![&stmt.body];
            blocks.extend(stmt.catch.as_deref());
            blocks.extend(stmt.finally.as_deref());
            blocks
        }
        StmtKind::Match(stmt) => {
            let mut blocks: Vec<&[Stmt]> =
                stmt.arms.iter().map(|arm| arm.body.as_slice()).collect();
            blocks.extend(stmt.default.as_deref());
            blocks
        }
        StmtKind::Parallel(stmt) => stmt.branches.iter().map(|b| b.body.as_slice()).collect(),
        StmtKind::Race(stmt) => stmt.branches.iter().map(|b| b.as_slice()).collect(),
        StmtKind::Mutex(stmt) => vec![&stmt.body],
        _ => Vec::new(),
    }
}

fn child_blocks_mut(kind: &mut StmtKind) -> Vec<&mut Vec<Stmt>> {
    match kind {
        StmtKind::If(stmt) => {
            let mut blocks: Vec<&mut Vec<Stmt>> =
                stmt.arms.iter_mut().map(|(_, body)| body).collect();
            blocks.extend(stmt.else_block.as_mut());
            blocks
        }
        StmtKind::For(stmt) => vec![&mut stmt.body],
        StmtKind::While(stmt) => vec![&mut stmt.body],
        StmtKind::Map(stmt) => vec![&mut stmt.body],
        StmtKind::Try(stmt) => {
            let mut blocks: Vec<&mut Vec<Stmt>> = vec![&mut stmt.body];
            blocks.extend(stmt.catch.as_mut());
            blocks.extend(stmt.finally.as_mut());
            blocks
        }
        StmtKind::Match(stmt) => {
            let mut blocks: Vec<&mut Vec<Stmt>> =
                stmt.arms.iter_mut().map(|arm| &mut arm.body).collect();
            blocks.extend(stmt.default.as_mut());
            blocks
        }
        StmtKind::Parallel(stmt) => stmt.branches.iter_mut().map(|b| &mut b.body).collect(),
        StmtKind::Race(stmt) => stmt.branches.iter_mut().collect(),
        StmtKind::Mutex(stmt) => vec![&mut stmt.body],
        _ => Vec::new(),
    }
}

fn rewrite_stmt(
    stmt: &mut Stmt,
    bindings: &HashMap<String, Expr>,
    renames: &HashMap<String, String>,
) {
    if let Some(label) = stmt.label.as_mut() {
        if let Some(renamed) = renames.get(label.as_str()) {
            *label = renamed.clone();
        }
    }
    rewrite_transitions(&mut stmt.transitions, bindings, renames);
    for expr in stmt_exprs_mut(&mut stmt.kind) {
        subst_expr(expr, bindings, renames);
    }
    for cond in stmt_conds_mut(&mut stmt.kind) {
        subst_cond(cond, bindings, renames);
    }
    for block in child_blocks_mut(&mut stmt.kind) {
        for child in block.iter_mut() {
            rewrite_stmt(child, bindings, renames);
        }
    }
}

fn rewrite_transitions(
    transitions: &mut TransitionClause,
    bindings: &HashMap<String, Expr>,
    renames: &HashMap<String, String>,
) {
    for slot in [
        &mut transitions.next,
        &mut transitions.on_success,
        &mut transitions.on_failure,
        &mut transitions.on_timeout,
        &mut transitions.on_reject,
    ] {
        rewrite_target(slot, renames);
    }
    for branch in transitions.branches.iter_mut() {
        subst_cond(&mut branch.when, bindings, renames);
        let mut target = Some(branch.target.clone());
        rewrite_target(&mut target, renames);
        if let Some(target) = target {
            branch.target = target;
        }
    }
}

fn rewrite_target(slot: &mut Option<Target>, renames: &HashMap<String, String>) {
    if let Some(Target::Label(label)) = slot {
        if let Some(renamed) = renames.get(label.as_str()) {
            *label = renamed.clone();
        }
    }
}

/// substitute a parameter reference, and namespace a reference to a label the region renamed.
///
/// a bare `p` becomes the argument expression outright; a `p.field` keeps the trailing path
/// segments when the argument is itself a path, and is otherwise left alone (a field access on a
/// literal has no spelling in the graph reference form).
fn subst_expr(
    expr: &mut Expr,
    bindings: &HashMap<String, Expr>,
    renames: &HashMap<String, String>,
) {
    if let ExprKind::Path(segs) = &expr.kind {
        if let Some(PathSeg::Key(head)) = segs.first() {
            if let Some(replacement) = bindings.get(head.as_str()) {
                if segs.len() == 1 {
                    let span = expr.span;
                    *expr = replacement.clone();
                    expr.span = span;
                    return;
                }
                if let ExprKind::Path(base) = &replacement.kind {
                    let mut merged = base.clone();
                    merged.extend(segs[1..].iter().cloned());
                    expr.kind = ExprKind::Path(merged);
                    return;
                }
            } else if let Some(renamed) = renames.get(head.as_str()) {
                let mut segs = segs.clone();
                segs[0] = PathSeg::Key(renamed.clone());
                expr.kind = ExprKind::Path(segs);
                return;
            }
        }
    }
    for child in expr_children_mut(&mut expr.kind) {
        subst_expr(child, bindings, renames);
    }
}

fn subst_cond(
    cond: &mut Cond,
    bindings: &HashMap<String, Expr>,
    renames: &HashMap<String, String>,
) {
    match &mut cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => {
            for part in parts.iter_mut() {
                subst_cond(part, bindings, renames);
            }
        }
        CondKind::Not(inner) => subst_cond(inner, bindings, renames),
        CondKind::Expr(expr) | CondKind::Exists(expr) => subst_expr(expr, bindings, renames),
        CondKind::Cmp { left, right, .. } => {
            subst_expr(left, bindings, renames);
            subst_expr(right, bindings, renames);
        }
    }
}

fn expr_children_mut(kind: &mut ExprKind) -> Vec<&mut Expr> {
    match kind {
        ExprKind::Array(items)
        | ExprKind::Concat(items)
        | ExprKind::Coalesce(items)
        | ExprKind::Add(items)
        | ExprKind::Sub(items)
        | ExprKind::Mul(items)
        | ExprKind::Div(items)
        | ExprKind::Mod(items) => items.iter_mut().collect(),
        ExprKind::Object(entries) => entries.iter_mut().map(|(_, value)| value).collect(),
        ExprKind::ToString(inner)
        | ExprKind::ToJson(inner)
        | ExprKind::Neg(inner)
        | ExprKind::Cast { expr: inner, .. } => vec![inner.as_mut()],
        ExprKind::Compare { left, right, .. } => vec![left.as_mut(), right.as_mut()],
        ExprKind::Ternary { cond, then, els } => {
            vec![cond.as_mut(), then.as_mut(), els.as_mut()]
        }
        ExprKind::Call {
            args,
            named,
            policy,
            ..
        } => {
            let mut out: Vec<&mut Expr> = args.iter_mut().collect();
            out.extend(named.iter_mut().map(|(_, value)| value));
            out.extend(policy.as_mut().map(|value| value.as_mut()));
            out
        }
        ExprKind::Lambda { body, .. } => vec![body.as_mut()],
        ExprKind::Apply { callee, args } => {
            let mut out = vec![callee.as_mut()];
            out.extend(args.iter_mut());
            out
        }
        ExprKind::Str(parts) => parts
            .iter_mut()
            .filter_map(|part| match part {
                StrPart::Expr(expr) => Some(expr),
                StrPart::Lit(_) => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// every expression a statement carries directly (not through a nested block).
fn stmt_exprs_mut(kind: &mut StmtKind) -> Vec<&mut Expr> {
    match kind {
        StmtKind::Action(action) => action.args.iter_mut().map(|(_, v)| v).collect(),
        StmtKind::TaskCall(call) => call.args.iter_mut().map(|(_, v)| v).collect(),
        StmtKind::Subflow(subflow) => {
            let mut out: Vec<&mut Expr> = subflow.params.iter_mut().map(|(_, v)| v).collect();
            out.extend(subflow.run_name.as_mut());
            out
        }
        StmtKind::Yield(value) => vec![value],
        StmtKind::Return(Some(value)) => vec![value],
        StmtKind::Fail(Some(value)) => vec![value],
        StmtKind::Transform(transform) => transform.bindings.iter_mut().map(|(_, v)| v).collect(),
        StmtKind::For(stmt) => vec![&mut stmt.items],
        StmtKind::Map(stmt) => vec![&mut stmt.items],
        _ => Vec::new(),
    }
}

fn stmt_conds_mut(kind: &mut StmtKind) -> Vec<&mut Cond> {
    match kind {
        StmtKind::If(stmt) => stmt.arms.iter_mut().map(|(cond, _)| cond).collect(),
        StmtKind::While(stmt) => vec![&mut stmt.cond],
        _ => Vec::new(),
    }
}

/// rewrite a statement sequence so runs of `async` launches become a `parallel` fan-out.
///
/// a launch group ends at the first statement that consumes one of its handles (`await`, `detach`,
/// or any expression referencing one); that statement is where the join lands. statements between
/// the launches and the join that touch none of the handles join the fan-out as a branch of their
/// own, so they keep overlapping with the launches instead of being pushed behind them.
///
/// a handle the block later `detach`es is excluded from the join's branch selector: the launch
/// still runs, but the run does not wait for it.
pub(super) fn group_async_launches(block: &[Stmt]) -> Vec<Stmt> {
    if !block.iter().any(|stmt| stmt.is_async) {
        return block.to_vec();
    }
    let detached = detached_handles(block);
    let mut out: Vec<Stmt> = Vec::with_capacity(block.len());
    let mut index = 0;
    while index < block.len() {
        if !block[index].is_async {
            out.push(block[index].clone());
            index += 1;
            continue;
        }
        let mut launches: Vec<Stmt> = Vec::new();
        let mut interleaved: Vec<Stmt> = Vec::new();
        let mut handles: Vec<String> = Vec::new();
        let mut cursor = index;
        while cursor < block.len() {
            let stmt = &block[cursor];
            if stmt.is_async {
                if let Some(label) = &stmt.label {
                    handles.push(label.clone());
                }
                launches.push(stmt.clone());
                cursor += 1;
                continue;
            }
            if consumes_any(stmt, &handles) {
                break;
            }
            interleaved.push(stmt.clone());
            cursor += 1;
        }
        // a lone launch with nothing to overlap is just a node; a fan-out of one buys nothing.
        if launches.len() == 1 && interleaved.is_empty() {
            out.push(launches.remove(0));
            index = cursor;
            continue;
        }
        let span = block[index].span;
        let mut branches: Vec<ParallelBranch> = launches
            .into_iter()
            .map(|stmt| ParallelBranch {
                label: stmt.label.clone(),
                body: vec![stmt],
            })
            .collect();
        if !interleaved.is_empty() {
            branches.push(ParallelBranch {
                label: None,
                body: interleaved,
            });
        }
        // wait only for the handles this block actually joins.
        let awaited: Vec<String> = handles
            .iter()
            .filter(|handle| !detached.contains(*handle))
            .cloned()
            .collect();
        let selected_branches = (awaited.len() != handles.len()).then_some(awaited);
        out.push(Stmt {
            span,
            annotations: Annotations::default(),
            label: None,
            label_type: None,
            kind: StmtKind::Parallel(ParallelStmt {
                branches,
                join: BranchPolicy::All,
                selected_branches,
            }),
            is_async: false,
            transitions: TransitionClause::default(),
            compensation: None,
            comments: CommentSet::default(),
        });
        index = cursor;
    }
    out
}

/// every handle the block drops with `detach`.
fn detached_handles(block: &[Stmt]) -> Vec<String> {
    block
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::Detach(handle) => Some(handle.clone()),
            _ => None,
        })
        .collect()
}

/// whether a statement consumes one of the pending handles — by awaiting it, detaching it, or
/// reading it in any expression or condition it carries.
fn consumes_any(stmt: &Stmt, handles: &[String]) -> bool {
    if handles.is_empty() {
        return false;
    }
    match &stmt.kind {
        StmtKind::Await(AwaitStmt {
            target: AwaitTarget::Task(task),
            ..
        }) => return handles.iter().any(|handle| handle == task),
        StmtKind::Detach(handle) => return handles.contains(handle),
        _ => {}
    }
    let mut probe = stmt.clone();
    let mut found = false;
    for expr in stmt_exprs_mut(&mut probe.kind) {
        if expr_reads_any(expr, handles) {
            found = true;
        }
    }
    if found {
        return true;
    }
    for cond in stmt_conds_mut(&mut probe.kind) {
        if cond_reads_any(cond, handles) {
            return true;
        }
    }
    false
}

fn expr_reads_any(expr: &mut Expr, handles: &[String]) -> bool {
    if let ExprKind::Path(segs) = &expr.kind {
        if let Some(PathSeg::Key(head)) = segs.first() {
            if handles.iter().any(|handle| handle == head) {
                return true;
            }
        }
    }
    expr_children_mut(&mut expr.kind)
        .into_iter()
        .any(|child| expr_reads_any(child, handles))
}

fn cond_reads_any(cond: &mut Cond, handles: &[String]) -> bool {
    match &mut cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => {
            parts.iter_mut().any(|part| cond_reads_any(part, handles))
        }
        CondKind::Not(inner) => cond_reads_any(inner, handles),
        CondKind::Expr(expr) | CondKind::Exists(expr) => expr_reads_any(expr, handles),
        CondKind::Cmp { left, right, .. } => {
            expr_reads_any(left, handles) || expr_reads_any(right, handles)
        }
    }
}
