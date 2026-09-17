#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RabbitMqBrokerConfig {
    pub uri: String,
    // second action queue carrying only `Labels`/`Replica`-targeted actions, so `Any` traffic (the
    // common case) never shares a queue with targeted traffic. See `Broker::receive_for`'s default
    // safety net: RabbitMQ can't natively express "message selector ⊆ consumer labels" for open-ended
    // labels, so this queue is deliberately coarse and every delivery from it is re-validated
    // client-side against the requesting profile before being handed back.
    pub control_queue: String,
    pub agent_queue_prefix: String,
    pub effect_queue: String,
    pub infrastructure_effect_queue: String,
    pub effect_result_queue: String,
    pub wake_queue: String,
    pub ingress_queue: String,
    // fan-out exchange for UI events; each subscriber binds its own exclusive queue.
    pub event_exchange: String,
    pub client_id: String,
    // per-consumer unacked delivery cap (basic.qos). without it rabbitmq pushes an entire backlog
    // to the first consumers, which then hold every message unacked while draining serially.
    pub prefetch_count: u16,
}

impl RabbitMqBrokerConfig {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            control_queue: DEFAULT_CONTROL_QUEUE.into(),
            agent_queue_prefix: DEFAULT_AGENT_QUEUE_PREFIX.into(),
            effect_queue: DEFAULT_EFFECT_QUEUE.into(),
            infrastructure_effect_queue: DEFAULT_INFRASTRUCTURE_EFFECT_QUEUE.into(),
            effect_result_queue: DEFAULT_EFFECT_RESULT_QUEUE.into(),
            wake_queue: DEFAULT_WAKE_QUEUE.into(),
            ingress_queue: DEFAULT_INGRESS_QUEUE.into(),
            event_exchange: DEFAULT_EVENT_EXCHANGE.into(),
            client_id: DEFAULT_CLIENT_ID.into(),
            prefetch_count: DEFAULT_PREFETCH_COUNT,
        }
    }

    /// override the per-consumer unacked delivery cap; 0 disables the limit.
    pub fn with_prefetch_count(mut self, prefetch_count: u16) -> Self {
        self.prefetch_count = prefetch_count;
        self
    }

    /// override the fan-out exchange used for UI events.
    pub fn with_event_exchange(mut self, event_exchange: impl Into<String>) -> Self {
        self.event_exchange = event_exchange.into();
        self
    }

    pub fn with_agent_queue_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.agent_queue_prefix = prefix.into();
        self
    }

    pub fn with_control_queue(mut self, control_queue: impl Into<String>) -> Self {
        self.control_queue = control_queue.into();
        self
    }

    /// override the orchestration queues (wake = WS -> waker, ingress = waker/worker -> WS).
    pub fn with_orchestration_queues(
        mut self,
        wake_queue: impl Into<String>,
        ingress_queue: impl Into<String>,
    ) -> Self {
        self.wake_queue = wake_queue.into();
        self.ingress_queue = ingress_queue.into();
        self
    }

    pub fn with_wake_queue(mut self, wake_queue: impl Into<String>) -> Self {
        self.wake_queue = wake_queue.into();
        self
    }

    pub fn with_ingress_queue(mut self, ingress_queue: impl Into<String>) -> Self {
        self.ingress_queue = ingress_queue.into();
        self
    }

    pub fn with_effect_queues(
        mut self,
        effect_queue: impl Into<String>,
        infrastructure_effect_queue: impl Into<String>,
        effect_result_queue: impl Into<String>,
    ) -> Self {
        self.effect_queue = effect_queue.into();
        self.infrastructure_effect_queue = infrastructure_effect_queue.into();
        self.effect_result_queue = effect_result_queue.into();
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn has_workflow_effect_queues(&self) -> bool {
        !self.effect_queue.trim().is_empty()
            && !self.infrastructure_effect_queue.trim().is_empty()
            && !self.effect_result_queue.trim().is_empty()
    }
}
