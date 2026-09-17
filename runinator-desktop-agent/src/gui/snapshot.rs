#[allow(unused_imports)]
use super::*;

pub(super) struct Snapshot {
    pub(super) status: AgentStatus,
    pub(super) connection: ConnectionState,
    pub(super) metrics: AgentMetrics,
    pub(super) busy: bool,
    /// which of Start / Cancel startup / Stop this phase warrants; see [`agent::Control`].
    pub(super) control: Control,
    pub(super) agent_activity: String,
    pub(super) agent_activity_age: Duration,
    pub(super) worker_activity: String,
    pub(super) worker_activity_age: Duration,
    pub(super) resources: Vec<agent::ResourceSample>,
    pub(super) execution_profiles: Vec<crate::execution_profiles::LocalProfileStatus>,
}
