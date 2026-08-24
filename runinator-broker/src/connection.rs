//! Topology selection above the concrete [`crate::Broker`] transports.
//!
//! A runtime can either connect to the broker network directly or use the authenticated web
//! service as a WebSocket relay.  Both paths eventually yield the same [`crate::Broker`] trait
//! object; keeping the choice here stops individual binaries from reimplementing URL rewriting,
//! credential forwarding, and the direct-vs-relay switch.

use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    build_broker_client, Broker, BrokerBuildError, BrokerClientConfig, BrokerConsumerProfile,
};

/// Default path of the authenticated broker relay on `runinator-ws`.
pub const DEFAULT_BROKER_RELAY_PATH: &str = "ws/broker";

/// The network topology a process uses to reach its broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrokerConnectionMode {
    /// Dial the configured concrete broker backend directly.
    #[default]
    Direct,
    /// Dial `runinator-ws` and forward broker calls over its authenticated WebSocket relay.
    Relay,
}

impl BrokerConnectionMode {
    /// Lowercase configuration spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Relay => "relay",
        }
    }

    /// Parse a command-line or environment spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "direct" => Some(Self::Direct),
            "relay" => Some(Self::Relay),
            _ => None,
        }
    }
}

/// A strategy for creating a broker client.
///
/// Implementations only decide *how a process reaches* the broker.  Once connected, callers use
/// the ordinary backend-neutral [`Broker`] API and do not need to know whether a message travelled
/// directly to Kafka/RabbitMQ/TCP or through a WebSocket relay.
#[async_trait]
pub trait BrokerConnection: Send + Sync {
    /// Resolve this topology to the concrete broker-client settings it will use.
    fn client_config(&self) -> Result<BrokerClientConfig, BrokerBuildError>;

    /// Human-readable path for status output and replica metadata.
    fn description(&self) -> Result<String, BrokerBuildError>;

    /// Build an instrumented broker for the requested channel profile.
    async fn connect(
        &self,
        profile: BrokerConsumerProfile,
    ) -> Result<Arc<dyn Broker>, BrokerBuildError> {
        let config = self.client_config()?;
        build_broker_client(&config, profile).await
    }
}

/// A direct connection to one of the concrete broker backends.
#[derive(Debug, Clone)]
pub struct DirectBrokerConnection {
    config: BrokerClientConfig,
}

impl DirectBrokerConnection {
    pub fn new(config: BrokerClientConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl BrokerConnection for DirectBrokerConnection {
    fn client_config(&self) -> Result<BrokerClientConfig, BrokerBuildError> {
        Ok(self.config.clone())
    }

    fn description(&self) -> Result<String, BrokerBuildError> {
        Ok(format!(
            "direct {} @ {}",
            self.config.backend, self.config.endpoint
        ))
    }
}

/// A connection which reaches the same broker through an authenticated `runinator-ws` relay.
#[derive(Debug, Clone)]
pub struct WebSocketRelayConnection {
    config: BrokerClientConfig,
    service_url: String,
    relay_path: String,
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

/// Pick a concrete connection strategy from normal process configuration.
///
/// `service_url` is ignored for a direct connection, so an application may retain an API URL that
/// is unusable as a WebSocket endpoint until it explicitly switches to relay mode.
pub fn select_broker_connection(
    mode: BrokerConnectionMode,
    config: BrokerClientConfig,
    service_url: impl Into<String>,
    relay_path: Option<&str>,
) -> Box<dyn BrokerConnection> {
    match mode {
        BrokerConnectionMode::Direct => Box::new(DirectBrokerConnection::new(config)),
        BrokerConnectionMode::Relay => Box::new(WebSocketRelayConnection::new(
            config,
            service_url,
            relay_path.unwrap_or(DEFAULT_BROKER_RELAY_PATH),
        )),
    }
}

/// Derive a WebSocket relay URL from a web-service base URL.
///
/// The join semantics preserve a path prefix: a service at
/// `https://example.test/runinator/` relays through
/// `wss://example.test/runinator/ws/broker`.
pub fn derive_websocket_relay_url(
    service_url: &str,
    relay_path: &str,
) -> Result<String, BrokerBuildError> {
    let mut url =
        url::Url::parse(service_url).map_err(|err| BrokerBuildError::InvalidEndpoint {
            endpoint: service_url.to_string(),
            message: err.to_string(),
        })?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => {
            return Err(BrokerBuildError::InvalidEndpoint {
                endpoint: service_url.to_string(),
                message: format!("unsupported service URL scheme '{other}'"),
            });
        }
    };
    url.set_scheme(scheme)
        .map_err(|_| BrokerBuildError::InvalidEndpoint {
            endpoint: service_url.to_string(),
            message: "cannot set WebSocket scheme".to_string(),
        })?;
    url.join(relay_path.trim_start_matches('/'))
        .map(|url| url.to_string())
        .map_err(|err| BrokerBuildError::InvalidEndpoint {
            endpoint: service_url.to_string(),
            message: err.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> BrokerClientConfig {
        BrokerClientConfig {
            backend: "tcp".into(),
            endpoint: "10.0.0.4:7070".into(),
            control_topic: "control".into(),
            agent_topic: Some("agent".into()),
            effect_topic: "effects".into(),
            infrastructure_effect_topic: "effects.infrastructure".into(),
            effect_result_topic: "effect-results".into(),
            client_id: "test".into(),
            relay_credential: Some("secret".into()),
            wake_topic: Some("wake".into()),
            ingress_topic: Some("ingress".into()),
        }
    }

    #[test]
    fn direct_connection_keeps_its_backend_and_ignores_service_url() {
        let connection = select_broker_connection(
            BrokerConnectionMode::Direct,
            config(),
            "ftp://not-used.example.test",
            None,
        );

        let resolved = connection.client_config().unwrap();
        assert_eq!(resolved.backend, "tcp");
        assert_eq!(resolved.endpoint, "10.0.0.4:7070");
        assert_eq!(
            connection.description().unwrap(),
            "direct tcp @ 10.0.0.4:7070"
        );
    }

    #[test]
    fn relay_connection_uses_websocket_and_preserves_path_prefix_and_credential() {
        let connection = select_broker_connection(
            BrokerConnectionMode::Relay,
            config(),
            "https://example.test/runinator/",
            None,
        );

        let resolved = connection.client_config().unwrap();
        assert_eq!(resolved.backend, "ws");
        assert_eq!(resolved.endpoint, "wss://example.test/runinator/ws/broker");
        assert_eq!(resolved.relay_credential.as_deref(), Some("secret"));
        assert_eq!(
            connection.description().unwrap(),
            "relay via wss://example.test/runinator/ws/broker"
        );
    }

    #[test]
    fn relay_connection_rejects_a_non_http_service_url() {
        let connection = select_broker_connection(
            BrokerConnectionMode::Relay,
            config(),
            "ftp://example.test",
            None,
        );

        assert!(matches!(
            connection.client_config(),
            Err(BrokerBuildError::InvalidEndpoint { .. })
        ));
    }

    #[test]
    fn connection_mode_round_trips_its_configuration_spelling() {
        for mode in [BrokerConnectionMode::Direct, BrokerConnectionMode::Relay] {
            assert_eq!(BrokerConnectionMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(
            BrokerConnectionMode::parse(" RELAY "),
            Some(BrokerConnectionMode::Relay)
        );
        assert_eq!(BrokerConnectionMode::parse("other"), None);
    }
}
