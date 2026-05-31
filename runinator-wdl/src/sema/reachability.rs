// reachability / dead-code, warning-only. a conservative structural pass: within any block,
// a statement that follows a terminator (a `fail`, or a step whose happy-path arrow diverts
// the linear successor) is unreachable unless it carries a label that some transition targets.
// it never fails compilation and never warns unless the successor is provably orphaned.

use std::collections::HashSet;

use crate::ast::*;

use super::{Diagnostic, child_blocks, effective_id};

pub(super) fn analyze(workflow: &Workflow, diagnostics: &mut Vec<Diagnostic>) {
    let mut targeted = HashSet::new();
    collect_targets(&workflow.body, &mut targeted);
    check_block(&workflow.body, &targeted, diagnostics);
}

fn check_block(block: &Block, targeted: &HashSet<String>, diagnostics: &mut Vec<Diagnostic>) {
    for (index, stmt) in block.iter().enumerate() {
        if index > 0 && terminates(&block[index - 1]) && !reachable_by_label(stmt, targeted) {
            diagnostics.push(Diagnostic::warning(
                stmt.span,
                "unreachable statement: the previous step never falls through to it",
            ));
        }
        for child in child_blocks(&stmt.kind) {
            check_block(child, targeted, diagnostics);
        }
    }
}

/// whether a statement diverts the linear happy path away from its textual successor.
fn terminates(stmt: &Stmt) -> bool {
    matches!(stmt.kind, StmtKind::Fail(_))
        || stmt.transitions.next.is_some()
        || stmt.transitions.on_success.is_some()
}

/// a successor is still reachable if it has a label that some transition jumps to.
fn reachable_by_label(stmt: &Stmt, targeted: &HashSet<String>) -> bool {
    effective_id(stmt).is_some_and(|id| targeted.contains(id))
}

fn collect_targets(block: &Block, targeted: &mut HashSet<String>) {
    for stmt in block {
        collect_clause(&stmt.transitions, targeted);
        if let StmtKind::Action(action) = &stmt.kind {
            if let Some(reentry) = &action.modifiers.reentry {
                if let Some(target) = &reentry.on_exhausted {
                    insert_target(target, targeted);
                }
            }
        }
        for child in child_blocks(&stmt.kind) {
            collect_targets(child, targeted);
        }
    }
}

fn collect_clause(clause: &TransitionClause, targeted: &mut HashSet<String>) {
    for target in [
        &clause.next,
        &clause.on_success,
        &clause.on_failure,
        &clause.on_timeout,
        &clause.on_reject,
    ]
    .into_iter()
    .flatten()
    {
        insert_target(target, targeted);
    }
}

fn insert_target(target: &Target, targeted: &mut HashSet<String>) {
    if let Target::Label(name) = target {
        targeted.insert(name.clone());
    }
}
