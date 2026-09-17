#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandSpec {
    pub path: Vec<String>,
    pub usage: String,
    pub summary: String,
    pub console_local: bool,
    pub arguments: Vec<ArgumentSpec>,
}
