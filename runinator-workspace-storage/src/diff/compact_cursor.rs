#[allow(unused_imports)]
use super::*;

#[derive(Serialize, Deserialize)]
pub(super) struct CompactCursor {
    pub(super) left: String,
    pub(super) right: String,
    pub(super) path: Option<String>,
    pub(super) emitted: bool,
    pub(super) after: Option<String>,
    pub(super) results_after: Option<String>,
    pub(super) done: bool,
}
