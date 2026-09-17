#[allow(unused_imports)]
use super::*;

/// what the output pane shows right now.
#[derive(Debug)]
pub(crate) struct Window<'a> {
    /// the visible lines, top to bottom.
    pub lines: Vec<&'a str>,
    /// one-based index of the first visible line, for the pane header.
    pub first: usize,
    /// how many lines are retained in total.
    pub total: usize,
    /// how many lines were discarded to stay under the limit.
    pub dropped: usize,
    /// true when the view is pinned to the newest output.
    pub following: bool,
    /// the first visible column.
    pub column: usize,
}
