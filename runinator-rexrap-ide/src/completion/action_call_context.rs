#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ActionCallContext {
    pub(crate) provider: String,
    pub(crate) action: String,
    pub(crate) replace_start: usize,
    pub(crate) replace_end: usize,
    pub(crate) used_args: BTreeSet<String>,
}
