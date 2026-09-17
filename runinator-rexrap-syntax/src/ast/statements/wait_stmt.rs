#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct WaitStmt {
    pub amount: WaitAmount,
    pub until_status: Option<String>,
    pub initial_status: Option<String>,
}
