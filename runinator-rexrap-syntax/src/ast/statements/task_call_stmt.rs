#[allow(unused_imports)]
use super::*;

/// `name(args)` where `name` is a `task fn`. the arguments are bound by substitution when the
/// body is inlined, so a call site never pays for a child run.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCallStmt {
    pub name: String,
    pub args: Vec<(String, Expr)>,
    pub modifiers: Modifiers,
}
