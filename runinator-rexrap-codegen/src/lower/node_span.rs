#[allow(unused_imports)]
use super::*;

/// One graph node paired with the source span of the statement that produced it.
///
/// Spans index the text the document was parsed from, so they are only meaningful alongside that
/// exact text — see `decompile_with_spans`, which returns both together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpan {
    pub node_id: String,
    pub start: usize,
    pub end: usize,
}
