#[allow(unused_imports)]
use super::*;

pub(super) struct Reservation<'a> {
    pub(super) cache: &'a ByteCache,
    pub(super) id: Id,
    pub(super) len: usize,
    pub(super) complete: bool,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.complete {
            return;
        }
        if let Ok(mut s) = self.cache.state.lock() {
            s.pending.remove(&self.id);
            s.reserved = s.reserved.saturating_sub(self.len);
        }
        self.cache.changed.notify_all();
    }
}
