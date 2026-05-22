pub mod adapters;
mod errors;
pub mod http;
pub mod in_memory;
pub mod tcp;
mod types;

pub use errors::BrokerError;
pub use runinator_comm::ControlCommand;
pub use types::{BrokerDelivery, BrokerMessage, ControlDelivery};

use async_trait::async_trait;

/// Trait implemented by queue backends capable of delivering task commands.
#[async_trait]
pub trait Broker: Send + Sync + 'static {
    /// Publish a message to the broker, optionally using a deduplication key.
    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next available delivery for the supplied consumer group.
    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError>;

    /// Acknowledge successful processing of a delivery.
    async fn ack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Return the delivery to the queue for another attempt.
    async fn nack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Publish a workflow control message on the control channel.
    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next control delivery for the supplied consumer group.
    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError>;

    /// Acknowledge successful processing of a control delivery.
    async fn ack_control(&self, consumer: &str, delivery_id: uuid::Uuid)
        -> Result<(), BrokerError>;
}
