#[allow(unused_imports)]
use super::*;

/// A definition candidate published by a successful console cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewConsoleFunction {
    pub name: String,
    pub is_task: bool,
    pub source: String,
}
