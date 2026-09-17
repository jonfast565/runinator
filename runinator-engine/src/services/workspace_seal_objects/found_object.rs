#[allow(unused_imports)]
use super::*;

pub(super) struct FoundObject {
    pub(super) pack: Arc<LocalPack>,
    pub(super) location: Location,
    pub(super) info: Option<ObjectInfo>,
}
