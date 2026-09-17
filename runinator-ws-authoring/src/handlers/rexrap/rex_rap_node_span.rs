#[allow(unused_imports)]
use super::*;

/// One graph node and the byte range of the statement that renders it, within the `source` returned
/// alongside it.
#[derive(Serialize)]
pub struct RexRapNodeSpan {
    pub node_id: String,
    pub start: usize,
    pub end: usize,
}
