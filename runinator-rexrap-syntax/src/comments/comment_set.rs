#[allow(unused_imports)]
use super::*;

/// the comments bound to one ast anchor: `leading` render on their own lines above it, `trailing`
/// renders as a suffix on the anchor's last line, and `dangling` render on their own lines after it
/// (used for comments trapped after the last statement of a block, before its closing brace).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommentSet {
    pub leading: Vec<Comment>,
    pub trailing: Option<Comment>,
    pub dangling: Vec<Comment>,
}

impl CommentSet {
    pub fn is_empty(&self) -> bool {
        self.leading.is_empty() && self.trailing.is_none() && self.dangling.is_empty()
    }
}
