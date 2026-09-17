#[allow(unused_imports)]
use super::*;

pub(crate) struct RequestGuard;

impl Drop for RequestGuard {
    fn drop(&mut self) {
        runinator_tui::gauge_increment("web service", "HTTP in flight", -1);
        metrics::gauge!(HTTP_IN_FLIGHT).decrement(1.0);
        handles().in_flight.add(-1, &[]);
    }
}
