use std::path::{Component, Path, PathBuf};

use crate::ast::*;
use crate::errors::WdlError;
use crate::parser::parse_document;

/// return the safe relative files referenced by `file("...")`, resolved against `source_dir`.
pub fn included_file_paths(src: &str, source_dir: &Path) -> Result<Vec<PathBuf>, WdlError> {
    let document = parse_document(src)?;
    let mut paths = Vec::new();
    collect_workflow(&document.workflow, source_dir, &mut paths)?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn collect_workflow(
    workflow: &Workflow,
    source_dir: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), WdlError> {
    if let Some(TypeExpr::Struct { fields, .. }) = &workflow.input {
        for field in fields {
            if let Some(default) = &field.default {
                collect_expr(default, source_dir, paths)?;
            }
        }
    }
    for trigger in &workflow.triggers {
        collect_expr(&trigger.schedule, source_dir, paths)?;
        if let Some(params) = &trigger.params {
            collect_expr(params, source_dir, paths)?;
        }
        for value in [&trigger.blackout_start, &trigger.blackout_end]
            .into_iter()
            .flatten()
        {
            collect_expr(value, source_dir, paths)?;
        }
    }
    for alias in &workflow.aliases {
        collect_entries(&alias.entries, source_dir, paths)?;
    }
    collect_block(&workflow.body, source_dir, paths)
}

fn collect_block(
    block: &Block,
    source_dir: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), WdlError> {
    for stmt in block {
        collect_stmt(stmt, source_dir, paths)?;
    }
    Ok(())
}

fn collect_stmt(stmt: &Stmt, source_dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), WdlError> {
    match &stmt.kind {
        StmtKind::Action(action) => collect_entries(&action.args, source_dir, paths)?,
        StmtKind::Compute(compute) => collect_compute_lines(&compute.body, source_dir, paths)?,
        StmtKind::Subflow(subflow) => {
            if let Some(run_name) = &subflow.run_name {
                collect_expr(run_name, source_dir, paths)?;
            }
            collect_entries(&subflow.params, source_dir, paths)?;
        }
        StmtKind::Wait(wait) => {
            if let WaitAmount::Expr(amount) = &wait.amount {
                collect_expr(amount, source_dir, paths)?;
            }
        }
        StmtKind::Output(output) => {
            if let Some(data) = &output.data {
                collect_expr(data, source_dir, paths)?;
            }
        }
        StmtKind::Input(input) => {
            if let Some(prompt) = &input.prompt {
                collect_expr(prompt, source_dir, paths)?;
            }
        }
        StmtKind::Approval(approval) => {
            collect_expr(&approval.prompt, source_dir, paths)?;
            collect_entries(&approval.metadata, source_dir, paths)?;
        }
        StmtKind::Config(config) => {
            if let Some(name) = &config.name {
                collect_expr(name, source_dir, paths)?;
            }
            if let Some(metadata) = &config.metadata {
                collect_expr(metadata, source_dir, paths)?;
            }
        }
        StmtKind::Fail(message) => {
            if let Some(message) = message {
                collect_expr(message, source_dir, paths)?;
            }
        }
        StmtKind::If(if_stmt) => {
            for (cond, body) in &if_stmt.arms {
                collect_cond(cond, source_dir, paths)?;
                collect_block(body, source_dir, paths)?;
            }
            if let Some(body) = &if_stmt.else_block {
                collect_block(body, source_dir, paths)?;
            }
        }
        StmtKind::For(for_stmt) => {
            collect_expr(&for_stmt.items, source_dir, paths)?;
            collect_block(&for_stmt.body, source_dir, paths)?;
        }
        StmtKind::While(while_stmt) => {
            collect_cond(&while_stmt.cond, source_dir, paths)?;
            collect_block(&while_stmt.body, source_dir, paths)?;
        }
        StmtKind::Match(match_stmt) => {
            collect_expr(&match_stmt.subject, source_dir, paths)?;
            for arm in &match_stmt.arms {
                if let Some(equals) = &arm.equals {
                    collect_expr(equals, source_dir, paths)?;
                }
                if let Some(when) = &arm.when {
                    collect_cond(when, source_dir, paths)?;
                }
                collect_block(&arm.body, source_dir, paths)?;
            }
            if let Some(body) = &match_stmt.default {
                collect_block(body, source_dir, paths)?;
            }
        }
        StmtKind::Parallel(parallel) => {
            for branch in &parallel.branches {
                collect_block(branch, source_dir, paths)?;
            }
        }
        StmtKind::Try(try_stmt) => {
            collect_block(&try_stmt.body, source_dir, paths)?;
            if let Some(body) = &try_stmt.catch {
                collect_block(body, source_dir, paths)?;
            }
            if let Some(body) = &try_stmt.finally {
                collect_block(body, source_dir, paths)?;
            }
        }
        StmtKind::Race(race) => {
            for branch in &race.branches {
                collect_block(branch, source_dir, paths)?;
            }
        }
        StmtKind::Map(map_stmt) => {
            collect_expr(&map_stmt.items, source_dir, paths)?;
            collect_block(&map_stmt.body, source_dir, paths)?;
        }
    }
    Ok(())
}

fn collect_compute_lines(
    lines: &[ComputeLine],
    source_dir: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), WdlError> {
    for line in lines {
        match line {
            ComputeLine::Let { value, .. }
            | ComputeLine::Return(value)
            | ComputeLine::Expr(value) => collect_expr(value, source_dir, paths)?,
            ComputeLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                collect_cond(cond, source_dir, paths)?;
                collect_compute_lines(then_branch, source_dir, paths)?;
                collect_compute_lines(else_branch, source_dir, paths)?;
            }
            ComputeLine::Goto(_) => {}
        }
    }
    Ok(())
}

fn collect_cond(cond: &Cond, source_dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), WdlError> {
    match &cond.kind {
        CondKind::All(conds) | CondKind::Any(conds) => {
            for cond in conds {
                collect_cond(cond, source_dir, paths)?;
            }
        }
        CondKind::Not(inner) => collect_cond(inner, source_dir, paths)?,
        CondKind::Expr(expr) => collect_expr(expr, source_dir, paths)?,
        CondKind::Cmp { left, right, .. } => {
            collect_expr(left, source_dir, paths)?;
            collect_expr(right, source_dir, paths)?;
        }
        CondKind::Exists(expr) => collect_expr(expr, source_dir, paths)?,
    }
    Ok(())
}

fn collect_entries(
    entries: &[(String, Expr)],
    source_dir: &Path,
    paths: &mut Vec<PathBuf>,
) -> Result<(), WdlError> {
    for (_, value) in entries {
        collect_expr(value, source_dir, paths)?;
    }
    Ok(())
}

fn collect_expr(expr: &Expr, source_dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), WdlError> {
    match &expr.kind {
        ExprKind::FileInclude { path } => {
            let relative = Path::new(path);
            if relative.as_os_str().is_empty() {
                return Err(WdlError::semantic(expr.span, "file() path cannot be empty"));
            }
            if !is_safe_relative_path(relative) {
                return Err(WdlError::semantic(
                    expr.span,
                    "file() path must be relative and cannot contain '..'",
                ));
            }
            paths.push(source_dir.join(relative));
        }
        ExprKind::DirInclude {
            path,
            recursive,
            max_depth,
        } => {
            let relative = Path::new(path);
            if relative.as_os_str().is_empty() {
                return Err(WdlError::semantic(expr.span, "dir() path cannot be empty"));
            }
            if !is_safe_relative_path(relative) {
                return Err(WdlError::semantic(
                    expr.span,
                    "dir() path must be relative and cannot contain '..'",
                ));
            }
            // best-effort bundling: list the directory so its files travel with the pack source.
            let base = source_dir.join(relative);
            if let Ok(entries) = dir_relative_files(&base, *recursive, *max_depth) {
                for entry in entries {
                    paths.push(base.join(entry));
                }
            }
        }
        ExprKind::Str(parts) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    collect_expr(inner, source_dir, paths)?;
                }
            }
        }
        ExprKind::Array(items)
        | ExprKind::Concat(items)
        | ExprKind::Coalesce(items)
        | ExprKind::Add(items)
        | ExprKind::Sub(items)
        | ExprKind::Mul(items)
        | ExprKind::Div(items)
        | ExprKind::Mod(items) => {
            for item in items {
                collect_expr(item, source_dir, paths)?;
            }
        }
        ExprKind::Object(entries) => collect_entries(entries, source_dir, paths)?,
        ExprKind::ToString(inner) | ExprKind::ToJson(inner) | ExprKind::Neg(inner) => {
            collect_expr(inner, source_dir, paths)?
        }
        ExprKind::Compare { left, right, .. } => {
            collect_expr(left, source_dir, paths)?;
            collect_expr(right, source_dir, paths)?;
        }
        ExprKind::Ternary { cond, then, els } => {
            collect_expr(cond, source_dir, paths)?;
            collect_expr(then, source_dir, paths)?;
            collect_expr(els, source_dir, paths)?;
        }
        ExprKind::Call { args, .. } => {
            for arg in args {
                collect_expr(arg, source_dir, paths)?;
            }
        }
        ExprKind::Lambda { body, .. } => collect_expr(body, source_dir, paths)?,
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::InlineCode { .. }
        | ExprKind::Path(_)
        | ExprKind::Spread(_) => {}
    }
    Ok(())
}

fn is_safe_relative_path(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// list files under `base` as paths relative to `base`, sorted for determinism. `recursive`
/// descends into subdirectories; `max_depth` caps how many directory levels are walked (`None` is
/// unlimited). top-level files always sit at depth 1.
pub(crate) fn dir_relative_files(
    base: &Path,
    recursive: bool,
    max_depth: Option<usize>,
) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk_dir(base, Path::new(""), 1, recursive, max_depth, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_dir(
    dir: &Path,
    prefix: &Path,
    depth: usize,
    recursive: bool,
    max_depth: Option<usize>,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let relative = prefix.join(entry.file_name());
        if file_type.is_file() {
            out.push(relative);
            continue;
        }
        // descend only when recursion is enabled and the depth cap allows another level.
        let within_cap = max_depth.map(|cap| depth < cap).unwrap_or(true);
        if file_type.is_dir() && recursive && within_cap {
            walk_dir(
                &entry.path(),
                &relative,
                depth + 1,
                recursive,
                max_depth,
                out,
            )?;
        }
    }
    Ok(())
}
