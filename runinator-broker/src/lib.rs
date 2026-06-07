pub mod adapters;
mod capabilities;
mod errors;
pub mod http;
pub mod in_memory;
pub mod tcp;
mod types;

pub use capabilities::{
    ensure_named_workflow_result_channel, ensure_workflow_result_channels_supported,
};
pub use errors::BrokerError;
pub use runinator_comm::{ControlCommand, UiEvent, WakeCommand, WsIngressCommand};
pub use types::{
    BrokerDelivery, BrokerMessage, ControlDelivery, EventDelivery, EventMessage, IngressDelivery,
    IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
};

use async_trait::async_trait;

/// Trait implemented by queue backends capable of delivering task commands.
#[async_trait]
pub trait Broker: Send + Sync + 'static {
    /// Report whether this backend supports workflow result channels.
    fn supports_workflow_result_channels(&self) -> bool {
        false
    }

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

    /// Publish a workflow result event on the result channel.
    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next workflow result delivery.
    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError>;

    /// Acknowledge successful processing of a workflow result delivery.
    async fn ack_result(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Return the workflow result delivery to the queue for another attempt.
    async fn nack_result(&self, consumer: &str, delivery_id: uuid::Uuid)
        -> Result<(), BrokerError>;

    /// Publish a delayed wake on the wake channel (web service -> waker).
    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next wake delivery for the supplied consumer group.
    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError>;

    /// Acknowledge successful processing of a wake delivery.
    async fn ack_wake(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Return the wake delivery to the queue for another attempt.
    async fn nack_wake(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Publish a message on the web-service ingress channel (waker/worker -> web service).
    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next ingress delivery for the supplied consumer group.
    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError>;

    /// Acknowledge successful processing of an ingress delivery.
    async fn ack_ingress(&self, consumer: &str, delivery_id: uuid::Uuid)
        -> Result<(), BrokerError>;

    /// Return the ingress delivery to the queue for another attempt.
    async fn nack_ingress(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError>;

    /// Publish a UI event on the fan-out `events` channel (web service -> every web-service replica).
    ///
    /// Unlike the other channels this is broadcast, not competing-consumer: every subscriber that
    /// has called [`Broker::receive_event`] receives its own copy. Best-effort, so there is no ack.
    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next UI event for the supplied subscriber.
    ///
    /// `consumer` identifies one fan-out subscriber (use a per-replica id); each distinct consumer
    /// drains its own stream of every published event.
    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError>;
}
