#[allow(unused_imports)]
use super::*;

/// where each band of the frame sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bands {
    pub status: Rect,
    /// the output band, top border included.
    pub output: Rect,
    /// the input band, top border included.
    pub input: Rect,
    pub menu: Rect,
    pub legend: Rect,
}

impl Bands {
    /// the rows of the output band that hold text, which is the pane height every scroll is in
    /// terms of.
    pub(crate) fn output_lines(&self) -> Rect {
        inner(self.output)
    }

    /// the pane the pointer at `position` is over, when it is over one that scrolls.
    pub(crate) fn pane_at(&self, position: Position) -> Option<Pane> {
        if self.output.contains(position) {
            return Some(Pane::Output);
        }
        if self.input.contains(position) || self.menu.contains(position) {
            return Some(Pane::Input);
        }
        None
    }
}
