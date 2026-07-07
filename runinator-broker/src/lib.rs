pub mod adapters;
mod capabilities;
pub mod dispatch;
mod errors;
mod factory;
pub mod http;
pub mod in_memory;
mod instrumented;
pub mod tcp;
#[cfg(test)]
mod tests;
mod types;
pub mod ws;

pub use capabilities::{
    ensure_named_workflow_result_channel, ensure_workflow_result_channels_supported,
};
pub use errors::BrokerError;
pub use factory::{build_kafka_broker, build_rabbitmq_broker};
pub use instrumented::instrument;
pub use runinator_comm::{
    ActionTarget, ConsumerProfile, ControlCommand, UiEvent, WakeCommand, WsIngressCommand,
};
pub use types::{
    BrokerDelivery, BrokerMessage, ControlDelivery, EventDelivery, EventMessage, IngressDelivery,
    IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
};

use async_trait::async_trait;

/// how long an undeliverable targeted control may bounce before a non-matching consumer drops it.
/// long enough to ride out a holder's broker reconnect, short enough to bound requeue churn once
/// the holder is truly gone.
pub const STALE_CONTROL_TTL_SECONDS: i64 = 300;

/// Trait implemented by queue backends capable of delivering task commands.
#[async_trait]
pub trait Broker: Send + Sync + 'static {
    /// Report whether this backend supports workflow result channels.
    fn supports_workflow_result_channels(&self) -> bool {
        false
    }

    /// Publish a message to the broker, optionally using a deduplication key.
    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next available delivery for the supplied consumer group. A plain
    /// consumer is treated as a general-pool ([`ConsumerProfile::shared`]) consumer, so it never
    /// receives replica- or label-targeted actions intended for a specific worker.
    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError>;

    /// Wait for and retrieve the next delivery whose target matches `profile`. The targeting-aware
    /// path: an exclusive consumer (e.g. the desktop worker) only receives `Replica`/`Labels`
    /// targets it satisfies, never general-pool `Any` work.
    ///
    /// Backends that do not have a smarter override (their own queue/topic routing per target) get
    /// this safety net for free: receive, check the delivery's target against `profile`, and requeue
    /// (`nack`) anything that doesn't match rather than handing it to the wrong consumer. A brief
    /// sleep between mismatches avoids a hot loop if nothing currently connected matches transiently;
    /// the reducer's own pre-dispatch liveness check means a genuine, lasting mismatch should be rare
    /// and will otherwise surface via the node's own timeout, not an unbounded spin here.
    async fn receive_for(&self, profile: &ConsumerProfile) -> Result<BrokerDelivery, BrokerError> {
        loop {
            let delivery = self.receive(&profile.id).await?;
            if delivery.command.target.matches(profile) {
                return Ok(delivery);
            }
            self.nack(&profile.id, delivery.delivery_id).await?;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Acknowledge successful processing of a delivery.
    async fn ack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Return the delivery to the queue for another attempt.
    async fn nack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError>;

    /// Publish a workflow control message on the control channel.
    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError>;

    /// Wait for and retrieve the next control delivery for the supplied consumer group,
    /// regardless of target (the legacy untargeted path).
    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError>;

    /// Wait for and retrieve the next control delivery whose target matches `profile`.
    ///
    /// The targeting-aware control path: the web service stamps cancels with the replica holding
    /// the action's executor lease, so the control reaches that worker instead of a random
    /// competing consumer (which would ack it after finding no matching local execution, losing
    /// the cancel). Backends without native routing get the same safety net as
    /// [`Broker::receive_for`]: receive, check the target, and requeue mismatches. Unlike an
    /// action, a control targeted at a replica that has since disconnected has no consumer left
    /// that can ever match it, so a mismatch older than [`STALE_CONTROL_TTL_SECONDS`] is acked
    /// (dropped) instead of requeued — controls are immediate signals and one that stale is moot.
    async fn receive_control_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<ControlDelivery, BrokerError> {
        loop {
            let delivery = self.receive_control(&profile.id).await?;
            if delivery.command.target.matches(profile) {
                return Ok(delivery);
            }
            let age = chrono::Utc::now() - delivery.enqueued_at;
            if age.num_seconds() >= STALE_CONTROL_TTL_SECONDS {
                self.ack_control(&profile.id, delivery.delivery_id).await?;
                continue;
            }
            self.nack_control(&profile.id, delivery.delivery_id).await?;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    /// Acknowledge successful processing of a control delivery.
    async fn ack_control(&self, consumer: &str, delivery_id: uuid::Uuid)
        -> Result<(), BrokerError>;

    /// Return the control delivery to the queue for another attempt (or for the consumer whose
    /// profile actually matches its target).
    async fn nack_control(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError>;

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
