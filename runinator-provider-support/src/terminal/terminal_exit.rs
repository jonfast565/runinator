#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalExit {
    pub success: bool,
    pub exit_code: i32,
}
