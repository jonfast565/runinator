#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy)]
pub(crate) struct WordAt<'a> {
    pub(crate) text: &'a str,
    pub(crate) start: usize,
    pub(crate) end: usize,
}
