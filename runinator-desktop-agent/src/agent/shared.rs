#[allow(unused_imports)]
use super::*;

/// state shared between the GUI thread and the background tokio runtime driving the agent.
#[derive(Default)]
pub struct Shared {
    pub status: AgentStatus,
    pub connection: ConnectionState,
    pub metrics: AgentMetrics,
    pub agent_activity: Activity,
    pub worker_activity: Activity,
    pub resource_history: ResourceHistory,
    pub busy: bool,
    pub logs: VecDeque<String>,
    pub execution_profiles: Vec<crate::execution_profiles::LocalProfileStatus>,
    // latch so one degraded episode fires exactly one "reconnecting" toast (and one "reconnected"
    // toast on recovery), rather than one per backoff retry.
    pub(super) degraded_notified: bool,
    // a separate latch, because giving up is a different event from retrying: an operator who has
    // already seen "reconnecting" still needs to be told the agent stopped.
    pub(super) disconnected_notified: bool,
    pub(super) handle: Option<AgentHandle>,
    // set for the window between a Start click and the lifecycle handle existing. `busy` alone
    // cannot say which transition is in flight, and only a start is cancellable.
    pub(super) starting: bool,
    // the in-flight `start_inner` task, kept so a cancel can abort it mid-registration rather than
    // leaving the operator to wait out a backoff that may never succeed.
    pub(super) start_task: Option<tokio::task::JoinHandle<()>>,
    // bumped by every start and every cancel. a startup compares it before publishing its handle, so
    // an abort that lands too late (or one issued before the task was even recorded) still cannot
    // leave a lifecycle running that the operator asked to cancel.
    pub(super) start_generation: u64,
}
