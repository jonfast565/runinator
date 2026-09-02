//! Durable, run-correlated observations at the engine's broker boundary.
//!
//! This is deliberately an engine wrapper rather than a broker backend feature: a broker remains
//! transport-neutral and workers never gain database access just to report diagnostics. The engine
//! sees every workflow-relevant publication and delivery it owns, which is enough to explain a
//! run's path across effect, result, wake, ingress, and control channels.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use runinator_broker_core::{
    AgentCommand, AgentDelivery, Broker, BrokerError, ConnectionState, ConsumerProfile,
    ControlCommand, ControlDelivery, EffectDelivery, EffectMessage, EffectResultDelivery,
    EffectResultMessage, EventDelivery, EventMessage, IngressDelivery, IngressMessage,
    WakeDelivery, WakeMessage, WsIngressCommand,
};
use runinator_models::{
    ingress_control::{BrokerMessageDirection, BrokerMessageRecord},
    value::Value,
};
use runinator_store::roles::DeliveryStore;
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

/// Wrap an engine's broker so operator-visible message records are persisted without affecting the
/// live delivery path if the diagnostic write fails.
pub(crate) struct TracingBroker<T> {
    inner: Arc<dyn Broker>,
    store: Arc<T>,
}

struct BrokerTraceCorrelation {
    workflow_run_id: Option<Uuid>,
    delivery_id: Option<Uuid>,
    dedupe_key: Option<String>,
    trace_id: Option<Uuid>,
}

impl<T: DeliveryStore> TracingBroker<T> {
    pub(crate) fn new(inner: Arc<dyn Broker>, store: Arc<T>) -> Self {
        Self { inner, store }
    }

    async fn record<M: Serialize + ?Sized>(
        &self,
        channel: &'static str,
        direction: BrokerMessageDirection,
        message_kind: &'static str,
        message: &M,
        correlation: BrokerTraceCorrelation,
    ) {
        let payload = serde_json::to_value(message)
            .map(Value::from)
            .unwrap_or_else(|error| {
                Value::String(format!("failed to serialize broker message: {error}"))
            });
        let record = BrokerMessageRecord {
            id: Uuid::now_v7(),
            channel: channel.into(),
            direction,
            message_kind: message_kind.into(),
            workflow_run_id: non_nil(correlation.workflow_run_id),
            delivery_id: correlation.delivery_id,
            dedupe_key: correlation.dedupe_key,
            trace_id: non_nil(correlation.trace_id),
            payload,
            occurred_at: Utc::now(),
        };
        if let Err(error) = self.store.record_broker_message(record).await {
            // Diagnostics must never turn a successful publish/receive into failed workflow work.
            warn!(error = %error, channel, message_kind, "failed to persist broker message trace");
        }
    }

    async fn record_effect_message(
        &self,
        direction: BrokerMessageDirection,
        message: &EffectMessage,
    ) {
        self.record(
            "effect",
            direction,
            "effect_command",
            message,
            BrokerTraceCorrelation {
                workflow_run_id: Some(message.command.workflow_run_id),
                delivery_id: None,
                dedupe_key: Some(message.dedupe_key_or_hash()),
                trace_id: Some(message.command.trace_id),
            },
        )
        .await;
    }

    async fn record_effect_delivery(&self, delivery: &EffectDelivery) {
        self.record(
            "effect",
            BrokerMessageDirection::Received,
            "effect_command",
            delivery,
            BrokerTraceCorrelation {
                workflow_run_id: Some(delivery.command.workflow_run_id),
                delivery_id: Some(delivery.delivery_id),
                dedupe_key: Some(delivery.dedupe_key.clone()),
                trace_id: Some(delivery.command.trace_id),
            },
        )
        .await;
    }

    async fn record_effect_result_message(
        &self,
        direction: BrokerMessageDirection,
        message: &EffectResultMessage,
    ) {
        self.record(
            "effect_result",
            direction,
            "effect_result",
            message,
            BrokerTraceCorrelation {
                workflow_run_id: Some(message.result.workflow_run_id),
                delivery_id: None,
                dedupe_key: Some(message.dedupe_key_or_hash()),
                trace_id: Some(message.result.trace_id),
            },
        )
        .await;
    }

    async fn record_effect_result_delivery(&self, delivery: &EffectResultDelivery) {
        self.record(
            "effect_result",
            BrokerMessageDirection::Received,
            "effect_result",
            delivery,
            BrokerTraceCorrelation {
                workflow_run_id: Some(delivery.result.workflow_run_id),
                delivery_id: Some(delivery.delivery_id),
                dedupe_key: Some(delivery.dedupe_key.clone()),
                trace_id: Some(delivery.result.trace_id),
            },
        )
        .await;
    }

    async fn record_wake_message(&self, direction: BrokerMessageDirection, message: &WakeMessage) {
        self.record(
            "wake",
            direction,
            "wake_command",
            message,
            BrokerTraceCorrelation {
                workflow_run_id: Some(message.command.workflow_run_id()),
                delivery_id: None,
                dedupe_key: Some(message.dedupe_key_or_hash()),
                trace_id: Some(message.command.trace_id),
            },
        )
        .await;
    }

    async fn record_wake_delivery(&self, delivery: &WakeDelivery) {
        self.record(
            "wake",
            BrokerMessageDirection::Received,
            "wake_command",
            delivery,
            BrokerTraceCorrelation {
                workflow_run_id: Some(delivery.command.workflow_run_id()),
                delivery_id: Some(delivery.delivery_id),
                dedupe_key: Some(delivery.dedupe_key.clone()),
                trace_id: Some(delivery.command.trace_id),
            },
        )
        .await;
    }

    async fn record_ingress_message(
        &self,
        direction: BrokerMessageDirection,
        message: &IngressMessage,
    ) {
        let (workflow_run_id, trace_id) = ingress_correlation(&message.command);
        self.record(
            "ingress",
            direction,
            ingress_kind(&message.command),
            message,
            BrokerTraceCorrelation {
                workflow_run_id,
                delivery_id: None,
                dedupe_key: Some(message.dedupe_key_or_hash()),
                trace_id,
            },
        )
        .await;
    }

    async fn record_ingress_delivery(&self, delivery: &IngressDelivery) {
        let (workflow_run_id, trace_id) = ingress_correlation(&delivery.command);
        self.record(
            "ingress",
            BrokerMessageDirection::Received,
            ingress_kind(&delivery.command),
            delivery,
            BrokerTraceCorrelation {
                workflow_run_id,
                delivery_id: Some(delivery.delivery_id),
                dedupe_key: Some(delivery.dedupe_key.clone()),
                trace_id,
            },
        )
        .await;
    }
}

fn non_nil(value: Option<Uuid>) -> Option<Uuid> {
    value.filter(|id| !id.is_nil())
}

fn ingress_kind(command: &WsIngressCommand) -> &'static str {
    match command {
        WsIngressCommand::SettleEffect { .. } => "settle_effect",
        WsIngressCommand::TimerInterrupt { .. } => "timer_interrupt",
        WsIngressCommand::OrchestrationIntent { .. } => "orchestration_intent",
        WsIngressCommand::Control { .. } => "control",
        WsIngressCommand::AgentDirectiveResult { .. } => "agent_directive_result",
        WsIngressCommand::ReplicaAvailability { .. } => "replica_availability",
    }
}

fn ingress_correlation(command: &WsIngressCommand) -> (Option<Uuid>, Option<Uuid>) {
    match command {
        WsIngressCommand::SettleEffect { result, trace_id } => {
            (Some(result.workflow_run_id), Some(*trace_id))
        }
        WsIngressCommand::TimerInterrupt {
            timer, trace_id, ..
        } => (Some(timer.workflow_run_id), Some(*trace_id)),
        WsIngressCommand::Control {
            workflow_run_id, ..
        } => (Some(*workflow_run_id), None),
        WsIngressCommand::OrchestrationIntent { trace_id, .. } => (None, Some(*trace_id)),
        WsIngressCommand::AgentDirectiveResult { .. }
        | WsIngressCommand::ReplicaAvailability { .. } => (None, None),
    }
}

#[async_trait]
impl<T: DeliveryStore> Broker for TracingBroker<T> {
    fn supports_workflow_effect_channels(&self) -> bool {
        self.inner.supports_workflow_effect_channels()
    }

    fn supports_agent_channel(&self) -> bool {
        self.inner.supports_agent_channel()
    }

    fn connection_state(&self) -> Option<tokio::sync::watch::Receiver<ConnectionState>> {
        self.inner.connection_state()
    }

    async fn heartbeat(&self) -> Result<(), BrokerError> {
        self.inner.heartbeat().await
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        let result = self.inner.publish_effect(message.clone()).await;
        if result.is_ok() {
            self.record_effect_message(BrokerMessageDirection::Published, &message)
                .await;
        }
        result
    }

    async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
        let result = self.inner.receive_effect(consumer).await;
        if let Ok(delivery) = &result {
            self.record_effect_delivery(delivery).await;
        }
        result
    }

    async fn receive_effect_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<EffectDelivery, BrokerError> {
        let result = self.inner.receive_effect_for(profile).await;
        if let Ok(delivery) = &result {
            self.record_effect_delivery(delivery).await;
        }
        result
    }

    async fn receive_infrastructure_effect(
        &self,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        let result = self.inner.receive_infrastructure_effect(consumer).await;
        if let Ok(delivery) = &result {
            self.record_effect_delivery(delivery).await;
        }
        result
    }

    async fn ack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_effect(consumer, delivery_id).await
    }

    async fn nack_effect(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_effect(consumer, delivery_id).await
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        let result = self.inner.publish_effect_result(message.clone()).await;
        if result.is_ok() {
            self.record_effect_result_message(BrokerMessageDirection::Published, &message)
                .await;
        }
        result
    }

    async fn receive_effect_result(
        &self,
        consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        let result = self.inner.receive_effect_result(consumer).await;
        if let Ok(delivery) = &result {
            self.record_effect_result_delivery(delivery).await;
        }
        result
    }

    async fn ack_effect_result(
        &self,
        consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        self.inner.ack_effect_result(consumer, delivery_id).await
    }

    async fn nack_effect_result(
        &self,
        consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        self.inner.nack_effect_result(consumer, delivery_id).await
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let result = self.inner.publish_control(command.clone()).await;
        if result.is_ok() {
            self.record(
                "control",
                BrokerMessageDirection::Published,
                "control_command",
                &command,
                BrokerTraceCorrelation {
                    workflow_run_id: Some(command.workflow_run_id),
                    delivery_id: None,
                    dedupe_key: None,
                    trace_id: None,
                },
            )
            .await;
        }
        result
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        let result = self.inner.receive_control(consumer).await;
        if let Ok(delivery) = &result {
            self.record(
                "control",
                BrokerMessageDirection::Received,
                "control_command",
                delivery,
                BrokerTraceCorrelation {
                    workflow_run_id: Some(delivery.command.workflow_run_id),
                    delivery_id: Some(delivery.delivery_id),
                    dedupe_key: None,
                    trace_id: None,
                },
            )
            .await;
        }
        result
    }

    async fn receive_control_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<ControlDelivery, BrokerError> {
        let result = self.inner.receive_control_for(profile).await;
        if let Ok(delivery) = &result {
            self.record(
                "control",
                BrokerMessageDirection::Received,
                "control_command",
                delivery,
                BrokerTraceCorrelation {
                    workflow_run_id: Some(delivery.command.workflow_run_id),
                    delivery_id: Some(delivery.delivery_id),
                    dedupe_key: None,
                    trace_id: None,
                },
            )
            .await;
        }
        result
    }

    async fn ack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_control(consumer, delivery_id).await
    }

    async fn nack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_control(consumer, delivery_id).await
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let result = self.inner.publish_agent(command.clone()).await;
        if result.is_ok() {
            self.record(
                "agent",
                BrokerMessageDirection::Published,
                "agent_command",
                &command,
                BrokerTraceCorrelation {
                    workflow_run_id: None,
                    delivery_id: None,
                    dedupe_key: Some(command.directive_id.to_string()),
                    trace_id: None,
                },
            )
            .await;
        }
        result
    }

    async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
        let result = self.inner.receive_agent(consumer).await;
        if let Ok(delivery) = &result {
            self.record(
                "agent",
                BrokerMessageDirection::Received,
                "agent_command",
                delivery,
                BrokerTraceCorrelation {
                    workflow_run_id: None,
                    delivery_id: Some(delivery.delivery_id),
                    dedupe_key: Some(delivery.command.directive_id.to_string()),
                    trace_id: None,
                },
            )
            .await;
        }
        result
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        let result = self.inner.receive_agent_for(profile).await;
        if let Ok(delivery) = &result {
            self.record(
                "agent",
                BrokerMessageDirection::Received,
                "agent_command",
                delivery,
                BrokerTraceCorrelation {
                    workflow_run_id: None,
                    delivery_id: Some(delivery.delivery_id),
                    dedupe_key: Some(delivery.command.directive_id.to_string()),
                    trace_id: None,
                },
            )
            .await;
        }
        result
    }

    async fn ack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_agent(consumer, delivery_id).await
    }

    async fn nack_agent(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_agent(consumer, delivery_id).await
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let result = self.inner.publish_wake(message.clone()).await;
        if result.is_ok() {
            self.record_wake_message(BrokerMessageDirection::Published, &message)
                .await;
        }
        result
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        let result = self.inner.receive_wake(consumer).await;
        if let Ok(delivery) = &result {
            self.record_wake_delivery(delivery).await;
        }
        result
    }

    async fn ack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_wake(consumer, delivery_id).await
    }

    async fn nack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_wake(consumer, delivery_id).await
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let result = self.inner.publish_ingress(message.clone()).await;
        if result.is_ok() {
            self.record_ingress_message(BrokerMessageDirection::Published, &message)
                .await;
        }
        result
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        let result = self.inner.receive_ingress(consumer).await;
        if let Ok(delivery) = &result {
            self.record_ingress_delivery(delivery).await;
        }
        result
    }

    async fn ack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_ingress(consumer, delivery_id).await
    }

    async fn nack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_ingress(consumer, delivery_id).await
    }

    // The event fan-out is a UI invalidation channel, not workflow transport. Keeping it out of
    // the run trace avoids filling an execution timeline with unrelated screen-refresh hints.
    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        self.inner.publish_event(message).await
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        self.inner.receive_event(consumer).await
    }
}
