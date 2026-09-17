#[allow(unused_imports)]
use super::*;

pub(crate) struct ProviderEffectRuntime {
    pub broker: Arc<dyn Broker>,
    pub profile: ConsumerProfile,
    pub libraries: Arc<HashMap<String, Plugin>>,
    pub api_client: AsyncApiClient<StaticLocator>,
    pub providers: ProviderFactory,
    pub max_concurrent_effects: usize,
    pub shutdown_grace: Duration,
    pub in_flight: Arc<Mutex<HashMap<Uuid, crate::worker::InFlightAction>>>,
    pub result_outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
    pub shutdown: Arc<Notify>,
    pub events: Arc<dyn crate::events::WorkerEventSink>,
    pub drained: Arc<AtomicBool>,
}
