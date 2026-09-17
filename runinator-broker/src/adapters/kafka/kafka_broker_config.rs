#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaBrokerConfig {
    pub bootstrap_servers: String,
    pub control_topic: String,
    pub agent_topic: String,
    pub effect_topic: String,
    pub infrastructure_effect_topic: String,
    pub effect_result_topic: String,
    pub wake_topic: String,
    pub ingress_topic: String,
    // fan-out: every subscriber uses a distinct group (keyed by consumer id) to read all events.
    pub event_topic: String,
    pub client_id: String,
}

impl KafkaBrokerConfig {
    pub fn new(bootstrap_servers: impl Into<String>) -> Self {
        Self {
            bootstrap_servers: bootstrap_servers.into(),
            control_topic: DEFAULT_CONTROL_TOPIC.into(),
            agent_topic: DEFAULT_AGENT_TOPIC.into(),
            effect_topic: DEFAULT_EFFECT_TOPIC.into(),
            infrastructure_effect_topic: DEFAULT_INFRASTRUCTURE_EFFECT_TOPIC.into(),
            effect_result_topic: DEFAULT_EFFECT_RESULT_TOPIC.into(),
            wake_topic: DEFAULT_WAKE_TOPIC.into(),
            ingress_topic: DEFAULT_INGRESS_TOPIC.into(),
            event_topic: DEFAULT_EVENT_TOPIC.into(),
            client_id: DEFAULT_CLIENT_ID.into(),
        }
    }

    /// override the fan-out topic used for UI events.
    pub fn with_event_topic(mut self, event_topic: impl Into<String>) -> Self {
        self.event_topic = event_topic.into();
        self
    }

    pub fn with_agent_topic(mut self, agent_topic: impl Into<String>) -> Self {
        self.agent_topic = agent_topic.into();
        self
    }

    pub fn with_control_topic(mut self, control_topic: impl Into<String>) -> Self {
        self.control_topic = control_topic.into();
        self
    }

    /// override the orchestration topics (wake = WS -> waker, ingress = waker/worker -> WS).
    pub fn with_orchestration_topics(
        mut self,
        wake_topic: impl Into<String>,
        ingress_topic: impl Into<String>,
    ) -> Self {
        self.wake_topic = wake_topic.into();
        self.ingress_topic = ingress_topic.into();
        self
    }

    pub fn with_wake_topic(mut self, wake_topic: impl Into<String>) -> Self {
        self.wake_topic = wake_topic.into();
        self
    }

    pub fn with_ingress_topic(mut self, ingress_topic: impl Into<String>) -> Self {
        self.ingress_topic = ingress_topic.into();
        self
    }

    pub fn with_effect_topics(
        mut self,
        effect_topic: impl Into<String>,
        infrastructure_effect_topic: impl Into<String>,
        effect_result_topic: impl Into<String>,
    ) -> Self {
        self.effect_topic = effect_topic.into();
        self.infrastructure_effect_topic = infrastructure_effect_topic.into();
        self.effect_result_topic = effect_result_topic.into();
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn has_workflow_effect_topics(&self) -> bool {
        !self.effect_topic.trim().is_empty()
            && !self.infrastructure_effect_topic.trim().is_empty()
            && !self.effect_result_topic.trim().is_empty()
    }
}
