#[allow(unused_imports)]
use super::*;

/// `resume [next|restart|fail]`. `None` is the bare form: resume at the interrupted node.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeStmt {
    pub mode: Option<String>,
}
