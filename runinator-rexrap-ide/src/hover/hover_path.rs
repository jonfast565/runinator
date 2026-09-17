#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(crate) struct HoverPath<'a> {
    pub(crate) parts: Vec<&'a str>,
    pub(crate) ranges: Vec<(usize, usize)>,
}

impl HoverPath<'_> {
    pub(super) fn segment_index_at(&self, offset: usize) -> Option<usize> {
        self.ranges
            .iter()
            .position(|(start, end)| *start <= offset && offset <= *end)
    }
}
