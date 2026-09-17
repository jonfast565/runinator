#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct HarnessEventBudget {
    pub(super) events: usize,
    pub(super) bytes: usize,
}

impl HarnessEventBudget {
    pub(super) fn admit(&mut self, bytes: usize) -> bool {
        if self.events >= MAX_HARNESS_EVENTS
            || self.bytes.saturating_add(bytes) > MAX_HARNESS_EVENT_BYTES
        {
            return false;
        }
        self.events += 1;
        self.bytes += bytes;
        true
    }
}
