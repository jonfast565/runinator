#[allow(unused_imports)]
use super::*;

pub struct AgentHandle {
    pub(super) shutdown: Shutdown,
    pub(super) task: JoinHandle<Result<(), SendableError>>,
    pub(super) state: watch::Receiver<AgentStatus>,
    pub(super) telemetry: Option<Arc<TelemetryCollector>>,
}

impl AgentHandle {
    /// request shutdown without waiting. safe to call more than once, and before the lifecycle has
    /// reached any particular stage.
    pub fn shutdown(&self) {
        self.shutdown.trigger();
    }

    /// await the lifecycle's own exit. returns what it returned: an error only when the agent could
    /// not be brought up at all.
    pub async fn wait(&mut self) -> Result<(), SendableError> {
        match (&mut self.task).await {
            Ok(result) => result,
            Err(err) if err.is_cancelled() => Ok(()),
            Err(err) => Err(crate::errors::LOOP_JOIN.error(err)),
        }
    }

    /// request shutdown and drain within `grace`, abandoning the task if it overruns. the worker
    /// loop bounds its own in-flight work, so an overrun means something below it is wedged.
    pub async fn stop(&mut self, grace: Duration) -> Result<(), SendableError> {
        self.shutdown.trigger();
        match tokio::time::timeout(grace, self.wait()).await {
            Ok(result) => result,
            Err(_) => {
                self.task.abort();
                Err(crate::errors::SHUTDOWN_TIMEOUT.error(format!("{}s", grace.as_secs())))
            }
        }
    }

    pub fn status(&self) -> AgentStatus {
        self.state.borrow().clone()
    }

    pub fn replica_id(&self) -> Option<Uuid> {
        self.state.borrow().replica_id
    }

    /// watch lifecycle transitions without implementing an observer.
    pub fn watch(&self) -> watch::Receiver<AgentStatus> {
        self.state.clone()
    }

    /// the host telemetry collector, when this agent samples it. exposed so a host can mirror
    /// cpu/memory at its own cadence rather than the heartbeat's.
    pub fn telemetry(&self) -> Option<Arc<TelemetryCollector>> {
        self.telemetry.clone()
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}
