#[allow(unused_imports)]
use super::*;

pub(super) struct DashboardSnapshot {
    pub(super) uptime: Duration,
    pub(super) components: Vec<ComponentSnapshot>,
    pub(super) logs: Vec<StyledLine>,
}
