#[allow(unused_imports)]
use super::*;

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
