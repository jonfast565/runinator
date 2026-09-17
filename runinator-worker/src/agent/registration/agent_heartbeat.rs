#[allow(unused_imports)]
use super::*;

pub(crate) struct AgentHeartbeat {
    pub(crate) broker: Arc<dyn Broker>,
    pub(crate) availability: AgentAvailability,
    pub(crate) heartbeat_interval: std::time::Duration,
    pub(crate) replica_id: Uuid,
    pub(crate) runtime_id: String,
    pub(crate) reporter: Arc<StatusReporter>,
    pub(crate) report_context: Arc<AgentReportContext>,
    pub(crate) telemetry: Option<Arc<TelemetryCollector>>,
    pub(crate) shutdown: Shutdown,
}
