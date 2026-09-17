#[allow(unused_imports)]
use super::*;

pub(super) struct RawOverlay<'a>(pub(super) &'a OverlayStore);

impl ReadStore for RawOverlay<'_> {
    fn info(&self, id: Id) -> Result<ObjectInfo> {
        self.0.raw_info(id)
    }
    fn get(&self, id: Id) -> Result<Object> {
        self.0.raw_get(id)
    }
}
