#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct TryStmt {
    pub body: Block,
    pub catch: Option<Block>,
    pub finally: Option<Block>,
}
