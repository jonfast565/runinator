// desugaring runs after parsing and before sema/lowering. it expands `...alias` spreads — in
// action arguments, object literals, subflow `with`, and approval metadata — into concrete entries
// using the workflow's header aliases, so the rest of the pipeline never sees a spread.

use std::collections::HashMap;

use crate::ast::*;
use crate::errors::{Span, WdlError};

pub(crate) type AliasTable = HashMap<String, Vec<(String, Expr)>>;

/// expand every `...alias` spread in the document. mutates the document in place.
pub fn desugar(document: &mut Document) -> Result<(), WdlError> {
    for workflow in document.workflows.iter_mut() {
        let aliases = collect_aliases(&workflow.aliases)?;
        expand_block(&mut workflow.body, &aliases)?;
    }
    Ok(())
}

/// splice the top-level `...alias` spreads in an authored entry list into a flat entry list
/// (positional last-wins), resolving alias composition. nested object spreads inside the
/// entry values are left intact for the caller to expand. used by lowering, which must build
/// the flat graph form while keeping the authored form for the resugar sidecar.
pub(crate) fn flatten_entries(
    entries: &[(String, Expr)],
    aliases: &AliasTable,
) -> Result<Vec<(String, Expr)>, WdlError> {
    let mut out: Vec<(String, Expr)> = Vec::new();
    for (key, value) in entries {
        if let ExprKind::Spread(name) = &value.kind {
            let resolved = resolve_alias(name, value.span, aliases, &mut Vec::new())?;
            for (key, value) in resolved {
                upsert(&mut out, key, value);
            }
            continue;
        }
        upsert(&mut out, key.clone(), value.clone());
    }
    Ok(out)
}

// build the alias lookup, rejecting duplicate names.
pub(crate) fn collect_aliases(aliases: &[Alias]) -> Result<AliasTable, WdlError> {
    let mut table = AliasTable::new();
    for alias in aliases {
        if table
            .insert(alias.name.clone(), alias.entries.clone())
            .is_some()
        {
            return Err(WdlError::semantic(
                alias.span,
                format!("duplicate alias '{}'", alias.name),
            ));
        }
    }
    Ok(table)
}

fn expand_block(block: &mut Block, aliases: &AliasTable) -> Result<(), WdlError> {
    for stmt in block.iter_mut() {
        expand_stmt(stmt, aliases)?;
    }
    Ok(())
}

// expand spreads inside a compute block's expressions, recursing into nested `if` branches.
fn expand_compute_block(body: &mut [ComputeLine], aliases: &AliasTable) -> Result<(), WdlError> {
    for line in body.iter_mut() {
        match line {
            ComputeLine::Let { value, .. }
            | ComputeLine::Return(value)
            | ComputeLine::Expr(value) => expand_expr(value, aliases)?,
            ComputeLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                expand_cond(cond, aliases)?;
                expand_compute_block(then_branch, aliases)?;
                expand_compute_block(else_branch, aliases)?;
            }
            ComputeLine::Goto(_) => {}
        }
    }
    Ok(())
}

// expand spreads in a statement's expressions and recurse into control-flow bodies.
fn expand_stmt(stmt: &mut Stmt, aliases: &AliasTable) -> Result<(), WdlError> {
    match &mut stmt.kind {
        StmtKind::Action(action) => expand_entries(&mut action.args, aliases)?,
        StmtKind::Compute(compute) => expand_compute_block(&mut compute.body, aliases)?,
        StmtKind::Subflow(subflow) => {
            if let Some(run_name) = subflow.run_name.as_mut() {
                expand_expr(run_name, aliases)?;
            }
            expand_entries(&mut subflow.params, aliases)?;
        }
        StmtKind::Approval(approval) => {
            expand_expr(&mut approval.prompt, aliases)?;
            expand_entries(&mut approval.metadata, aliases)?;
        }
        StmtKind::Gate(gate) => {
            if let Some(when) = gate.when.as_mut() {
                expand_cond(when, aliases)?;
            }
            expand_entries(&mut gate.metadata, aliases)?;
        }
        StmtKind::Signal(signal) => {
            expand_entries(&mut signal.metadata, aliases)?;
        }
        StmtKind::Config(config) => {
            if let Some(name) = config.name.as_mut() {
                expand_expr(name, aliases)?;
            }
            if let Some(metadata) = config.metadata.as_mut() {
                expand_expr(metadata, aliases)?;
            }
        }
        StmtKind::Output(output) => {
            if let Some(data) = output.data.as_mut() {
                expand_expr(data, aliases)?;
            }
            for (_, source) in output.items.iter_mut() {
                expand_expr(source, aliases)?;
            }
        }
        StmtKind::Yield(value) => expand_expr(value, aliases)?,
        StmtKind::Input(input) => {
            if let Some(prompt) = input.prompt.as_mut() {
                expand_expr(prompt, aliases)?;
            }
        }
        StmtKind::Wait(wait) => {
            if let WaitAmount::Expr(expr) = &mut wait.amount {
                expand_expr(expr, aliases)?;
            }
        }
        StmtKind::Fail(expr) => {
            if let Some(expr) = expr.as_mut() {
                expand_expr(expr, aliases)?;
            }
        }
        StmtKind::If(if_stmt) => {
            for (cond, body) in if_stmt.arms.iter_mut() {
                expand_cond(cond, aliases)?;
                expand_block(body, aliases)?;
            }
            if let Some(body) = if_stmt.else_block.as_mut() {
                expand_block(body, aliases)?;
            }
        }
        StmtKind::For(for_stmt) => {
            expand_expr(&mut for_stmt.items, aliases)?;
            expand_block(&mut for_stmt.body, aliases)?;
        }
        StmtKind::While(while_stmt) => {
            expand_cond(&mut while_stmt.cond, aliases)?;
            expand_block(&mut while_stmt.body, aliases)?;
        }
        StmtKind::Map(map_stmt) => {
            expand_expr(&mut map_stmt.items, aliases)?;
            expand_block(&mut map_stmt.body, aliases)?;
        }
        StmtKind::Match(match_stmt) => {
            expand_expr(&mut match_stmt.subject, aliases)?;
            for arm in match_stmt.arms.iter_mut() {
                if let Some(equals) = arm.equals.as_mut() {
                    expand_expr(equals, aliases)?;
                }
                if let Some(when) = arm.when.as_mut() {
                    expand_cond(when, aliases)?;
                }
                expand_block(&mut arm.body, aliases)?;
            }
            if let Some(body) = match_stmt.default.as_mut() {
                expand_block(body, aliases)?;
            }
        }
        StmtKind::Parallel(parallel) => {
            for branch in parallel.branches.iter_mut() {
                expand_block(branch, aliases)?;
            }
        }
        StmtKind::Race(race) => {
            for branch in race.branches.iter_mut() {
                expand_block(branch, aliases)?;
            }
        }
        StmtKind::Try(try_stmt) => {
            expand_block(&mut try_stmt.body, aliases)?;
            if let Some(body) = try_stmt.catch.as_mut() {
                expand_block(body, aliases)?;
            }
            if let Some(body) = try_stmt.finally.as_mut() {
                expand_block(body, aliases)?;
            }
        }
        StmtKind::Assert(assert) => {
            for (_, cond) in assert.assertions.iter_mut() {
                expand_cond(cond, aliases)?;
            }
        }
        StmtKind::Transform(transform) => {
            for (_, value) in transform.bindings.iter_mut() {
                expand_expr(value, aliases)?;
            }
        }
        StmtKind::Audit(audit) => {
            expand_expr(&mut audit.action, aliases)?;
            for value in [
                audit.actor.as_mut(),
                audit.target.as_mut(),
                audit.reason.as_mut(),
            ]
            .into_iter()
            .flatten()
            {
                expand_expr(value, aliases)?;
            }
        }
        StmtKind::Await(await_stmt) => {
            expand_expr(&mut await_stmt.run_ids, aliases)?;
        }
        StmtKind::Debounce(debounce) => {
            if let Some(key) = debounce.key.as_mut() {
                expand_expr(key, aliases)?;
            }
        }
        StmtKind::EventSource(es) => {
            if let Some(filter) = es.filter.as_mut() {
                expand_cond(filter, aliases)?;
            }
        }
        StmtKind::Mutex(mutex) => {
            expand_block(&mut mutex.body, aliases)?;
        }
        // these carry no spread-bearing expressions.
        StmtKind::Checkpoint(_)
        | StmtKind::Throttle(_)
        | StmtKind::Collect(_)
        | StmtKind::Barrier(_)
        | StmtKind::CircuitBreaker(_) => {}
    }
    Ok(())
}

fn expand_cond(cond: &mut Cond, aliases: &AliasTable) -> Result<(), WdlError> {
    match &mut cond.kind {
        CondKind::All(conds) | CondKind::Any(conds) => {
            for cond in conds.iter_mut() {
                expand_cond(cond, aliases)?;
            }
        }
        CondKind::Not(inner) => expand_cond(inner, aliases)?,
        CondKind::Expr(expr) => expand_expr(expr, aliases)?,
        CondKind::Cmp { left, right, .. } => {
            expand_expr(left, aliases)?;
            expand_expr(right, aliases)?;
        }
        CondKind::Exists(expr) => expand_expr(expr, aliases)?,
    }
    Ok(())
}

// recurse into an expression, expanding spreads inside nested object literals.
fn expand_expr(expr: &mut Expr, aliases: &AliasTable) -> Result<(), WdlError> {
    match &mut expr.kind {
        ExprKind::Object(entries) => expand_entries(entries, aliases)?,
        ExprKind::Array(items) => {
            for item in items.iter_mut() {
                expand_expr(item, aliases)?;
            }
        }
        ExprKind::Concat(parts) | ExprKind::Coalesce(parts) => {
            for part in parts.iter_mut() {
                expand_expr(part, aliases)?;
            }
        }
        ExprKind::ToString(inner) | ExprKind::ToJson(inner) | ExprKind::Neg(inner) => {
            expand_expr(inner, aliases)?
        }
        ExprKind::Compare { left, right, .. } => {
            expand_expr(left, aliases)?;
            expand_expr(right, aliases)?;
        }
        ExprKind::Ternary { cond, then, els } => {
            expand_expr(cond, aliases)?;
            expand_expr(then, aliases)?;
            expand_expr(els, aliases)?;
        }
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts.iter_mut() {
                expand_expr(part, aliases)?;
            }
        }
        ExprKind::Call { args, .. } => {
            for arg in args.iter_mut() {
                expand_expr(arg, aliases)?;
            }
        }
        ExprKind::Lambda { body, .. } => expand_expr(body, aliases)?,
        ExprKind::Str(parts) => {
            for part in parts.iter_mut() {
                if let StrPart::Expr(part) = part {
                    expand_expr(part, aliases)?;
                }
            }
        }
        // a bare spread expression is only ever produced inside an entry list and handled by
        // `expand_entries`; encountering one as a standalone value is a grammar/parser invariant
        // violation.
        ExprKind::Spread(name) => {
            return Err(WdlError::semantic(
                expr.span,
                format!("spread '...{name}' is not allowed here"),
            ));
        }
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::FileInclude { .. }
        | ExprKind::DirInclude { .. }
        | ExprKind::InlineCode { .. }
        | ExprKind::Path(_) => {}
    }
    Ok(())
}

// rebuild an entry list in source order, splicing each spread's resolved entries in place and
// letting later entries override earlier ones of the same key (positional last-wins).
fn expand_entries(entries: &mut Vec<(String, Expr)>, aliases: &AliasTable) -> Result<(), WdlError> {
    let mut out: Vec<(String, Expr)> = Vec::new();
    for (key, mut value) in entries.drain(..) {
        if let ExprKind::Spread(name) = &value.kind {
            let resolved = resolve_alias(name, value.span, aliases, &mut Vec::new())?;
            for (key, value) in resolved {
                upsert(&mut out, key, value);
            }
            continue;
        }
        expand_expr(&mut value, aliases)?;
        upsert(&mut out, key, value);
    }
    *entries = out;
    Ok(())
}

// resolve an alias to a flat entry list, expanding nested spreads (aliases may compose other
// aliases) while detecting reference cycles.
fn resolve_alias(
    name: &str,
    span: Span,
    aliases: &AliasTable,
    visiting: &mut Vec<String>,
) -> Result<Vec<(String, Expr)>, WdlError> {
    if visiting.iter().any(|seen| seen == name) {
        return Err(WdlError::semantic(
            span,
            format!("alias '{name}' references itself"),
        ));
    }
    let entries = aliases
        .get(name)
        .ok_or_else(|| WdlError::semantic(span, format!("unknown alias '{name}'")))?;
    visiting.push(name.to_string());
    let mut out: Vec<(String, Expr)> = Vec::new();
    for (key, value) in entries {
        if let ExprKind::Spread(inner) = &value.kind {
            let resolved = resolve_alias(inner, value.span, aliases, visiting)?;
            for (key, value) in resolved {
                upsert(&mut out, key, value);
            }
            continue;
        }
        let mut value = value.clone();
        expand_expr(&mut value, aliases)?;
        upsert(&mut out, key.clone(), value);
    }
    visiting.pop();
    Ok(out)
}

// insert a key, or overwrite its value in place to preserve first-seen ordering.
fn upsert(list: &mut Vec<(String, Expr)>, key: String, value: Expr) {
    if let Some(slot) = list.iter_mut().find(|(existing, _)| *existing == key) {
        slot.1 = value;
        return;
    }
    list.push((key, value));
}
