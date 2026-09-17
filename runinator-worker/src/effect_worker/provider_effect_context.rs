#[allow(unused_imports)]
use super::*;

pub(super) struct ProviderEffectContext {
    pub(super) broker: Arc<dyn Broker>,
    pub(super) consumer: String,
    pub(super) executor_replica_id: Option<Uuid>,
    pub(super) libraries: Arc<HashMap<String, Plugin>>,
    pub(super) api_client: AsyncApiClient<StaticLocator>,
    pub(super) providers: ProviderFactory,
    pub(super) cache: Arc<FunctionCache>,
    pub(super) in_flight: Arc<Mutex<HashMap<Uuid, crate::worker::InFlightAction>>>,
    pub(super) result_outbox: Arc<dyn crate::agent::outbox::ResultOutbox>,
    pub(super) events: Arc<dyn crate::events::WorkerEventSink>,
}
