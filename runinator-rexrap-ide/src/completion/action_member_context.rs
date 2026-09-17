#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ActionMemberContext {
    pub(crate) provider: String,
    pub(crate) replace_start: usize,
}
