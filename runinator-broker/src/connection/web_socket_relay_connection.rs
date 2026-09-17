#[allow(unused_imports)]
use super::*;

/// A connection which reaches the same broker through an authenticated `runinator-ws` relay.
#[derive(Debug, Clone)]
pub struct WebSocketRelayConnection {
    pub(super) config: BrokerClientConfig,
    pub(super) service_url: String,
    pub(super) relay_path: String,
}

impl WebSocketRelayConnection {
    /// `config.relay_credential` is forwarded as the relay's bearer credential.
    pub fn new(
        config: BrokerClientConfig,
        service_url: impl Into<String>,
        relay_path: impl Into<String>,
    ) -> Self {
        Self {
            config,
            service_url: service_url.into(),
            relay_path: relay_path.into(),
        }
    }
}

#[async_trait]
impl BrokerConnection for WebSocketRelayConnection {
    fn client_config(&self) -> Result<BrokerClientConfig, BrokerBuildError> {
        let endpoint = derive_websocket_relay_url(&self.service_url, &self.relay_path)?;
        let mut config = self.config.clone();
        config.backend = "ws".to_string();
        config.endpoint = endpoint;
        Ok(config)
    }

    fn description(&self) -> Result<String, BrokerBuildError> {
        Ok(format!("relay via {}", self.client_config()?.endpoint))
    }
}
