// desugaring runs after parsing and before sema/lowering. it expands `...alias` argument spreads
// in action calls into concrete arguments using the workflow's header aliases, so the rest of the
// pipeline only ever sees fully-resolved argument lists and never needs to know about aliases.

use std::collections::HashMap;

use crate::ast::*;
use crate::errors::WdlError;

type AliasTable = HashMap<String, Vec<(String, Expr)>>;

/// expand every `...alias` spread in the document's action calls. mutates the document in place.
pub fn desugar(document: &mut Document) -> Result<(), WdlError> {
    let aliases = collect_aliases(&document.workflow.aliases)?;
    expand_block(&mut document.workflow.body, &aliases)
}

// build the alias lookup, rejecting duplicate names.
fn collect_aliases(aliases: &[Alias]) -> Result<AliasTable, WdlError> {
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

// recurse into control-flow bodies; expand spreads on action statements.
fn expand_stmt(stmt: &mut Stmt, aliases: &AliasTable) -> Result<(), WdlError> {
    match &mut stmt.kind {
        StmtKind::Action(action) => expand_action(action, aliases),
        StmtKind::If(if_stmt) => {
            for (_, body) in if_stmt.arms.iter_mut() {
                expand_block(body, aliases)?;
            }
            if let Some(body) = if_stmt.else_block.as_mut() {
                expand_block(body, aliases)?;
            }
            Ok(())
        }
        StmtKind::For(stmt) => expand_block(&mut stmt.body, aliases),
        StmtKind::While(stmt) => expand_block(&mut stmt.body, aliases),
        StmtKind::Map(stmt) => expand_block(&mut stmt.body, aliases),
        StmtKind::Match(stmt) => {
            for arm in stmt.arms.iter_mut() {
                expand_block(&mut arm.body, aliases)?;
            }
            if let Some(body) = stmt.default.as_mut() {
                expand_block(body, aliases)?;
            }
            Ok(())
        }
        StmtKind::Parallel(stmt) => {
            for branch in stmt.branches.iter_mut() {
                expand_block(branch, aliases)?;
            }
            Ok(())
        }
        StmtKind::Race(stmt) => {
            for branch in stmt.branches.iter_mut() {
                expand_block(branch, aliases)?;
            }
            Ok(())
        }
        StmtKind::Try(stmt) => {
            expand_block(&mut stmt.body, aliases)?;
            if let Some(body) = stmt.catch.as_mut() {
                expand_block(body, aliases)?;
            }
            if let Some(body) = stmt.finally.as_mut() {
                expand_block(body, aliases)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// merge each referenced alias's entries (in listed order) ahead of the explicit args, with explicit
// args overriding spread entries regardless of position.
fn expand_action(action: &mut ActionStmt, aliases: &AliasTable) -> Result<(), WdlError> {
    if action.arg_spreads.is_empty() {
        return Ok(());
    }
    let mut merged: Vec<(String, Expr)> = Vec::new();
    for (name, span) in &action.arg_spreads {
        let entries = aliases
            .get(name)
            .ok_or_else(|| WdlError::semantic(*span, format!("unknown alias '{name}'")))?;
        for (key, value) in entries {
            upsert(&mut merged, key.clone(), value.clone());
        }
    }
    for (key, value) in action.args.drain(..) {
        upsert(&mut merged, key, value);
    }
    action.args = merged;
    action.arg_spreads.clear();
    Ok(())
}

// insert a key, or overwrite its value in place to preserve first-seen ordering.
fn upsert(list: &mut Vec<(String, Expr)>, key: String, value: Expr) {
    if let Some(slot) = list.iter_mut().find(|(existing, _)| *existing == key) {
        slot.1 = value;
        return;
    }
    list.push((key, value));
}
