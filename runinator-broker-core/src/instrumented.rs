// Records OpenTelemetry metrics for each broker operation.
// The `backend` tag separates throughput and latency for each backend. Without OpenTelemetry
// configuration, this wrapper does nothing.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use opentelemetry::metrics::{Counter, Histogram, ObservableGauge};
use opentelemetry::KeyValue;

use crate::types::{
    AgentDelivery, ConnectionState, ControlDelivery, EffectDelivery, EffectMessage,
    EffectResultDelivery, EffectResultMessage, EventDelivery, EventMessage, IngressDelivery,
    IngressMessage, WakeDelivery, WakeMessage,
};
use crate::{AgentCommand, Broker, BrokerError, ConsumerProfile, ControlCommand};

const METER_NAME: &str = "runinator-broker";
const METRIC_OPERATIONS: &str = "runinator_broker_operations_total";
const METRIC_DURATION_MS: &str = "runinator_broker_operation_duration_ms";

// channel names used as the `channel` attribute; they mirror the broker's logical channels.
const CH_CONTROL: &str = "control";
const CH_AGENT: &str = "agent";
const CH_EFFECT: &str = "effect";
const CH_EFFECT_RESULT: &str = "effect_result";
const CH_WAKE: &str = "wake";
const CH_INGRESS: &str = "ingress";
const CH_EVENT: &str = "events";
const CH_CONNECTION: &str = "connection";

/// wrap `inner` so its operations emit otel metrics tagged with `backend`. the returned broker is a
/// drop-in for the wrapped one; when otel is disabled the meter is a no-op and this adds only a
/// per-call timestamp read.
pub fn instrument(inner: Arc<dyn Broker>, backend: impl Into<String>) -> Arc<dyn Broker> {
    let backend = backend.into();
    let connection_state = inner.connection_state();
    Arc::new(InstrumentedBroker {
        inner,
        metrics: BrokerMetrics::new(backend, connection_state),
    })
}

struct BrokerMetrics {
    backend: String,
    operations: Counter<u64>,
    duration_ms: Histogram<f64>,
    _connection_state: ObservableGauge<u64>,
}

impl BrokerMetrics {
    fn new(
        backend: String,
        connection_state: Option<tokio::sync::watch::Receiver<ConnectionState>>,
    ) -> Self {
        let meter = opentelemetry::global::meter(METER_NAME);
        let callback_backend = backend.clone();
        let connection_state = meter
            .u64_observable_gauge("runinator_broker_connection_state")
            .with_callback(move |observer| {
                let connected = connection_state
                    .as_ref()
                    .is_none_or(|state| state.borrow().is_connected());
                observer.observe(
                    u64::from(connected),
                    &[KeyValue::new("backend", callback_backend.clone())],
                );
            })
            .build();
        Self {
            backend,
            operations: meter.u64_counter(METRIC_OPERATIONS).build(),
            duration_ms: meter
                .f64_histogram(METRIC_DURATION_MS)
                .with_unit("ms")
                .build(),
            _connection_state: connection_state,
        }
    }

    // record a completed operation. every call increments the operations counter tagged with the
    // outcome; `timed` operations (non-blocking publishes and acks) also record their latency, while
    // blocking receives are left untimed so the histogram never conflates idle wait with work.
    fn record<T>(
        &self,
        channel: &'static str,
        op: &'static str,
        start: Instant,
        result: &Result<T, BrokerError>,
        timed: bool,
    ) {
        let outcome = if result.is_ok() { "ok" } else { "error" };
        self.operations.add(
            1,
            &[
                KeyValue::new("backend", self.backend.clone()),
                KeyValue::new("channel", channel),
                KeyValue::new("op", op),
                KeyValue::new("outcome", outcome),
            ],
        );
        if timed {
            let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
            self.duration_ms.record(
                elapsed_ms,
                &[
                    KeyValue::new("backend", self.backend.clone()),
                    KeyValue::new("channel", channel),
                    KeyValue::new("op", op),
                ],
            );
        }
    }
}

struct InstrumentedBroker {
    inner: Arc<dyn Broker>,
    metrics: BrokerMetrics,
}

#[async_trait]
impl Broker for InstrumentedBroker {
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
        let start = Instant::now();
        let result = self.inner.heartbeat().await;
        self.metrics
            .record(CH_CONNECTION, "heartbeat", start, &result, true);
        result
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_control(command).await;
        self.metrics
            .record(CH_CONTROL, "publish", start, &result, true);
        result
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_control(consumer).await;
        self.metrics
            .record(CH_CONTROL, "receive", start, &result, false);
        result
    }

    async fn receive_control_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<ControlDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_control_for(profile).await;
        self.metrics
            .record(CH_CONTROL, "receive", start, &result, false);
        result
    }

    async fn ack_control(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_control(consumer, delivery_id).await;
        self.metrics.record(CH_CONTROL, "ack", start, &result, true);
        result
    }

    async fn nack_control(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_control(consumer, delivery_id).await;
        self.metrics
            .record(CH_CONTROL, "nack", start, &result, true);
        result
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_agent(command).await;
        self.metrics
            .record(CH_AGENT, "publish", start, &result, true);
        result
    }

    async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_agent(consumer).await;
        self.metrics
            .record(CH_AGENT, "receive", start, &result, false);
        result
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_agent_for(profile).await;
        self.metrics
            .record(CH_AGENT, "receive", start, &result, false);
        result
    }

    async fn ack_agent(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_agent(consumer, delivery_id).await;
        self.metrics.record(CH_AGENT, "ack", start, &result, true);
        result
    }

    async fn nack_agent(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_agent(consumer, delivery_id).await;
        self.metrics.record(CH_AGENT, "nack", start, &result, true);
        result
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_effect(message).await;
        self.metrics
            .record(CH_EFFECT, "publish", start, &result, true);
        result
    }

    async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_effect(consumer).await;
        self.metrics
            .record(CH_EFFECT, "receive", start, &result, false);
        result
    }

    async fn receive_effect_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<EffectDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_effect_for(profile).await;
        self.metrics
            .record(CH_EFFECT, "receive", start, &result, false);
        result
    }

    async fn receive_infrastructure_effect(
        &self,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_infrastructure_effect(consumer).await;
        self.metrics
            .record(CH_EFFECT, "receive", start, &result, false);
        result
    }

    async fn ack_effect(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_effect(consumer, delivery_id).await;
        self.metrics.record(CH_EFFECT, "ack", start, &result, true);
        result
    }

    async fn nack_effect(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_effect(consumer, delivery_id).await;
        self.metrics.record(CH_EFFECT, "nack", start, &result, true);
        result
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_effect_result(message).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "publish", start, &result, true);
        result
    }

    async fn receive_effect_result(
        &self,
        consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_effect_result(consumer).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "receive", start, &result, false);
        result
    }

    async fn ack_effect_result(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_effect_result(consumer, delivery_id).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "ack", start, &result, true);
        result
    }

    async fn nack_effect_result(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_effect_result(consumer, delivery_id).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "nack", start, &result, true);
        result
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_wake(message).await;
        self.metrics
            .record(CH_WAKE, "publish", start, &result, true);
        result
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_wake(consumer).await;
        self.metrics
            .record(CH_WAKE, "receive", start, &result, false);
        result
    }

    async fn ack_wake(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_wake(consumer, delivery_id).await;
        self.metrics.record(CH_WAKE, "ack", start, &result, true);
        result
    }

    async fn nack_wake(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_wake(consumer, delivery_id).await;
        self.metrics.record(CH_WAKE, "nack", start, &result, true);
        result
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_ingress(message).await;
        self.metrics
            .record(CH_INGRESS, "publish", start, &result, true);
        result
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_ingress(consumer).await;
        self.metrics
            .record(CH_INGRESS, "receive", start, &result, false);
        result
    }

    async fn ack_ingress(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_ingress(consumer, delivery_id).await;
        self.metrics.record(CH_INGRESS, "ack", start, &result, true);
        result
    }

    async fn nack_ingress(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_ingress(consumer, delivery_id).await;
        self.metrics
            .record(CH_INGRESS, "nack", start, &result, true);
        result
    }

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_event(message).await;
        self.metrics
            .record(CH_EVENT, "publish", start, &result, true);
        result
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_event(consumer).await;
        self.metrics
            .record(CH_EVENT, "receive", start, &result, false);
        result
    }
}
