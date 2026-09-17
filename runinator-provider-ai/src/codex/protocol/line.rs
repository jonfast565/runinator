#[allow(unused_imports)]
use super::*;

pub(crate) struct Line {
    pub(super) stream: &'static str,
    pub(super) content: String,
    pub(super) truncated: bool,
}
