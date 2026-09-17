#[allow(unused_imports)]
use super::*;

pub(super) struct Cursor {
    pub(super) comments: Vec<Comment>,
    pub(super) i: usize,
}

impl Cursor {
    pub(super) fn peek(&self) -> Option<&Comment> {
        self.comments.get(self.i)
    }

    pub(super) fn take(&mut self) -> Comment {
        let comment = self.comments[self.i].clone();
        self.i += 1;
        comment
    }
}
