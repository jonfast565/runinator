#[allow(unused_imports)]
use super::*;

/// the inputs that decide which broker connection strategy a host uses. resolved through
/// [`runinator_broker::BrokerConnection`] into a [`BrokerConfig`] plus a human description, which
/// is what both hosts display.
#[derive(Debug, Clone)]
pub struct BrokerSelection {
    pub mode: BrokerMode,
    /// used to derive the relay URL in [`BrokerMode::Relay`]; ignored in `Direct`.
    pub service_url: String,
    /// Backend name (`tcp`/`http`/`rabbitmq`/`kafka`/`in-memory`), used only in `Direct`.
    pub direct_backend: String,
    /// backend endpoint, only used in `Direct`.
    pub direct_endpoint: String,
    pub control_topic: String,
    pub agent_topic: String,
    pub effect_topic: String,
    pub infrastructure_effect_topic: String,
    pub effect_result_topic: String,
    /// The broker ingress path that carries this worker's lifecycle observations to the engine.
    pub ingress_topic: String,
    pub client_id: String,
    pub api_key: Option<String>,
}

impl BrokerSelection {
    /// resolve to the broker config to build, and a description such as
    /// `relay via wss://host/ws/broker` or `direct tcp @ 10.0.0.4:7070`.
    pub fn resolve(self) -> Result<(BrokerConfig, String), SendableError> {
        let connection = select_broker_connection(
            self.mode,
            BrokerClientConfig {
                backend: self.direct_backend,
                endpoint: self.direct_endpoint,
                control_topic: self.control_topic,
                agent_topic: Some(self.agent_topic),
                effect_topic: self.effect_topic,
                infrastructure_effect_topic: self.infrastructure_effect_topic,
                effect_result_topic: self.effect_result_topic,
                client_id: self.client_id,
                relay_credential: self.api_key,
                wake_topic: None,
                ingress_topic: Some(self.ingress_topic),
            },
            self.service_url,
            None,
        );
        let client = connection.client_config()?;
        let description = connection.description()?;

        Ok((
            BrokerConfig {
                broker_backend: client.backend,
                broker_endpoint: client.endpoint,
                broker_effect_topic: client.effect_topic,
                broker_infrastructure_effect_topic: client.infrastructure_effect_topic,
                broker_control_topic: client.control_topic,
                broker_agent_topic: client.agent_topic.unwrap_or_default(),
                broker_effect_result_topic: client.effect_result_topic,
                broker_ingress_topic: client.ingress_topic.unwrap_or_default(),
                broker_client_id: client.client_id,
                api_key: client.relay_credential,
            },
            description,
        ))
    }
}
