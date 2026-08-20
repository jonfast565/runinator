//! the broker contract and its backend-independent pieces.
//!
//! this crate holds what every participant in the runtime needs to *talk about* a broker: the
//! [`Broker`] trait, the per-channel message/delivery types, [`BrokerError`], the channel-capability
//! checks, the otel wrapper, and the in-memory backend. it deliberately excludes the concrete
//! transports and external adapters (http, tcp, ws, kafka, rabbitmq) — those live in
//! `runinator-broker`, which depends on this crate.
//!
//! a caller that only publishes and consumes through a `dyn Broker` (the web-service handler crates,
//! `runinator-engine`) should depend on this crate. only the binaries that *build* a concrete
//! backend need `runinator-broker`.

mod capabilities;
mod errors;
pub mod in_memory;
mod instrumented;
#[cfg(test)]
mod tests;
mod types;

pub use capabilities::{
    ensure_agent_channel_supported, ensure_named_workflow_result_channel,
    ensure_workflow_result_channels_supported,
};
pub use errors::BrokerError;
pub use instrumented::instrument;
pub use runinator_comm::{
    ActionTarget, AgentCommand, AgentDirectiveKind, AgentDirectiveResult, AgentDirectiveStatus,
    ConsumerProfile, ControlCommand, UiEvent, WakeCommand, WsIngressCommand,
};
pub use types::{
    AgentDelivery, BrokerDelivery, BrokerMessage, ConnectionState, ControlDelivery, EffectDelivery,
    EffectMessage, EffectResultDelivery, EffectResultMessage, EventDelivery, EventMessage,
    IngressDelivery, IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
};

use async_trait::async_trait;

/// how long an undeliverable targeted control may bounce before a non-matching consumer drops it.
/// long enough to ride out a holder's broker reconnect, short enough to bound requeue churn once
/// the holder is truly gone.
pub const STALE_CONTROL_TTL_SECONDS: i64 = 300;

/// Trait implemented by queue backends capable of delivering task commands.
#[async_trait]
pub trait Broker: Send + Sync + 'static {
    /// Whether this backend can carry the VM's generic effect protocol. The legacy action/result
    /// channel is intentionally separate: a VM run must never be reconstructed as a node action.
    fn supports_workflow_effect_channels(&self) -> bool {
        false
    }
    /// Report whether this backend supports workflow result channels.
    fn supports_workflow_result_channels(&self) -> bool {
        false
    }

    fn supports_agent_channel(&self) -> bool {
        false
    }

    /// Observe this backend's connection to its broker, if it owns one it re-establishes itself.
    ///
    /// `None` for every backend whose connectivity is either not a thing (in-memory) or handled per
    /// request (tcp/http dial each call), so there is no steady state to watch. The `ws` relay
    /// overrides it: it holds one long-lived connection across reconnects, which makes "connected"
    /// a real, observable property a host wants to display rather than infer from log lines.
    fn connection_state(&self) -> Option<tokio::sync::watch::Receiver<ConnectionState>> {
        None
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

    /// Publish a generic workflow VM effect command.
    async fn publish_effect(&self, _message: EffectMessage) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("publish_effect"))
    }

    async fn receive_effect(&self, _consumer: &str) -> Result<EffectDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("receive_effect"))
    }

    async fn receive_effect_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<EffectDelivery, BrokerError> {
        loop {
            let delivery = self.receive_effect(&profile.id).await?;
            if delivery.command.target.matches(profile) {
                return Ok(delivery);
            }
            self.nack_effect(&profile.id, delivery.delivery_id).await?;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    async fn ack_effect(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("ack_effect"))
    }

    async fn nack_effect(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("nack_effect"))
    }

    /// Publish the terminal/streaming result of a generic VM effect.
    async fn publish_effect_result(
        &self,
        _message: EffectResultMessage,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("publish_effect_result"))
    }

    async fn receive_effect_result(
        &self,
        _consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("receive_effect_result"))
    }

    async fn ack_effect_result(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("ack_effect_result"))
    }

    async fn nack_effect_result(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("nack_effect_result"))
    }

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

    /// Publish a replica-scoped fleet directive on the agent channel.
    async fn publish_agent(&self, _command: AgentCommand) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("publish_agent"))
    }

    async fn receive_agent(&self, _consumer: &str) -> Result<AgentDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("receive_agent"))
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        loop {
            let delivery = self.receive_agent(&profile.id).await?;
            if delivery.command.target.matches(profile) {
                return Ok(delivery);
            }
            self.nack_agent(&profile.id, delivery.delivery_id).await?;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    async fn ack_agent(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("ack_agent"))
    }

    async fn nack_agent(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("nack_agent"))
    }

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
