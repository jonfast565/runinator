#[allow(unused_imports)]
use super::*;

/// `await workflow "name" …` or `await task_name …`: wait for durable work to reach a terminal
/// state. Task joins identify one exact run rather than scanning every run of a workflow.
#[derive(Debug, Clone, PartialEq)]
pub struct AwaitStmt {
    pub target: AwaitTarget,
    pub key: Option<Expr>,
    pub mode: Option<String>,
    pub timeout: Option<i64>,
}
