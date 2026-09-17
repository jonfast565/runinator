#[allow(unused_imports)]
use super::*;

pub(crate) struct InFlightGuard;

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        runinator_tui::gauge_increment("worker", "effects in flight", -1);
        metrics().effects_in_flight.add(-1, &[]);
    }
}
