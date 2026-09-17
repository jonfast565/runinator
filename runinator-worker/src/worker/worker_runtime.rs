#[allow(unused_imports)]
use super::*;

pub struct WorkerRuntime {
    pub broker: Arc<dyn Broker>,
    pub profile: ConsumerProfile,
    pub libraries: Arc<HashMap<String, Plugin>>,
    pub api_client: AsyncApiClient<StaticLocator>,
    pub providers: ProviderFactory,
    pub max_concurrent_actions: usize,
    pub shutdown_grace: Duration,
    pub shutdown: Arc<Notify>,
    pub events: Arc<dyn WorkerEventSink>,
    pub result_outbox: Arc<dyn ResultOutbox>,
    pub directive_handler: Arc<dyn DirectiveHandler>,
}
