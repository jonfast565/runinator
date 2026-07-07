// a backend-neutral instrumentation decorator that records opentelemetry metrics for every broker
// operation, tagged with the concrete `backend` so per-broker throughput and latency can be broken
// out (in-memory/http/tcp/kafka/rabbitmq). it delegates all behavior to the wrapped broker and is a
// no-op unless otel is configured, so it can wrap any backend unconditionally.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use opentelemetry::metrics::{Counter, Histogram};
use opentelemetry::KeyValue;

use crate::types::{
    BrokerDelivery, BrokerMessage, ControlDelivery, EventDelivery, EventMessage, IngressDelivery,
    IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
};
use crate::{Broker, BrokerError, ConsumerProfile, ControlCommand};

const METER_NAME: &str = "runinator-broker";
const METRIC_OPERATIONS: &str = "runinator_broker_operations_total";
const METRIC_DURATION_MS: &str = "runinator_broker_operation_duration_ms";

// channel names used as the `channel` attribute; they mirror the broker's logical channels.
const CH_ACTION: &str = "action";
const CH_CONTROL: &str = "control";
const CH_RESULT: &str = "result";
const CH_WAKE: &str = "wake";
const CH_INGRESS: &str = "ingress";
const CH_EVENT: &str = "events";

/// wrap `inner` so its operations emit otel metrics tagged with `backend`. the returned broker is a
/// drop-in for the wrapped one; when otel is disabled the meter is a no-op and this adds only a
/// per-call timestamp read.
pub fn instrument(inner: Arc<dyn Broker>, backend: impl Into<String>) -> Arc<dyn Broker> {
    Arc::new(InstrumentedBroker {
        inner,
        metrics: BrokerMetrics::new(backend.into()),
    })
}

struct BrokerMetrics {
    backend: String,
    operations: Counter<u64>,
    duration_ms: Histogram<f64>,
}

impl BrokerMetrics {
    fn new(backend: String) -> Self {
        let meter = opentelemetry::global::meter(METER_NAME);
        Self {
            backend,
            operations: meter.u64_counter(METRIC_OPERATIONS).build(),
            duration_ms: meter
                .f64_histogram(METRIC_DURATION_MS)
                .with_unit("ms")
                .build(),
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
    fn supports_workflow_result_channels(&self) -> bool {
        self.inner.supports_workflow_result_channels()
    }

    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish(message).await;
        self.metrics
            .record(CH_ACTION, "publish", start, &result, true);
        result
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive(consumer).await;
        self.metrics
            .record(CH_ACTION, "receive", start, &result, false);
        result
    }

    async fn receive_for(&self, profile: &ConsumerProfile) -> Result<BrokerDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_for(profile).await;
        self.metrics
            .record(CH_ACTION, "receive", start, &result, false);
        result
    }

    async fn ack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack(consumer, delivery_id).await;
        self.metrics.record(CH_ACTION, "ack", start, &result, true);
        result
    }

    async fn nack(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack(consumer, delivery_id).await;
        self.metrics.record(CH_ACTION, "nack", start, &result, true);
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

    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_result(message).await;
        self.metrics
            .record(CH_RESULT, "publish", start, &result, true);
        result
    }

    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_result(consumer).await;
        self.metrics
            .record(CH_RESULT, "receive", start, &result, false);
        result
    }

    async fn ack_result(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_result(consumer, delivery_id).await;
        self.metrics.record(CH_RESULT, "ack", start, &result, true);
        result
    }

    async fn nack_result(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_result(consumer, delivery_id).await;
        self.metrics.record(CH_RESULT, "nack", start, &result, true);
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
