#[allow(unused_imports)]
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Frame {
    pub(super) name: String,
    pub(super) left: Option<Id>,
    pub(super) right: Option<Id>,
    pub(super) emitted: bool,
    pub(super) after: Option<Vec<u8>>,
}
