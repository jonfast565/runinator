#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct PathContext {
    pub(crate) head: String,
    pub(crate) completed: Vec<String>,
    pub(crate) replace_start: usize,
    pub(crate) replace_end: usize,
}
