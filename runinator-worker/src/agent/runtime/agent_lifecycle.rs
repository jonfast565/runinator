#[allow(unused_imports)]
use super::*;

pub(super) struct AgentLifecycle {
    pub(super) config: AgentRuntimeConfig,
    pub(super) api_client: AsyncApiClient<StaticLocator>,
    pub(super) libraries: Arc<std::collections::HashMap<String, runinator_plugin::plugin::Plugin>>,
    pub(super) telemetry: Option<Arc<TelemetryCollector>>,
    pub(super) report_context: Arc<AgentReportContext>,
    pub(super) result_outbox: Arc<dyn ResultOutbox>,
    pub(super) reporter: Arc<StatusReporter>,
    pub(super) shutdown: Shutdown,
}
