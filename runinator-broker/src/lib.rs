pub mod adapters;
mod errors;
pub mod http;
pub mod in_memory;
mod types;

pub use errors::BrokerError;
pub use types::{BrokerDelivery, BrokerMessage};

use async_trait::async_trait;

/// Trait implemented by queue backends capable of delivering task commands.
#[async_trait]
pub trait Broker: Send + Sync + 'static {
    /// Publish a message to the broker, optionally using a deduplication key.
    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError>;

    /// Retrieve the next available delivery for the supplied consumer group.
    async fn poll(&self, consumer: &str) -> Result<Option<BrokerDelivery>, BrokerError>;

    /// Acknowledge successful processing of a delivery.
    async fn ack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Return the delivery to the queue for another attempt.
    async fn nack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;
}
