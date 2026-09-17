#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct ExecResult {
    pub(super) result: String,
    pub(super) thread_id: Option<String>,
    pub(super) usage: Value,
}
