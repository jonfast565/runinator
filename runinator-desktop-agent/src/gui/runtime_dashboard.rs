#[allow(unused_imports)]
use super::*;

pub(super) struct RuntimeDashboard<'a> {
    pub(super) status: &'a AgentStatus,
    pub(super) metrics: &'a AgentMetrics,
    pub(super) agent_activity: &'a str,
    pub(super) agent_activity_age: Duration,
    pub(super) worker_activity: &'a str,
    pub(super) worker_activity_age: Duration,
    pub(super) resources: &'a [agent::ResourceSample],
    pub(super) uptime: Duration,
    pub(super) capacity: usize,
    pub(super) service_url: &'a str,
}
