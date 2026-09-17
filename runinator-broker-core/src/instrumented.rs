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

mod broker_metrics;
use broker_metrics::BrokerMetrics;

mod instrumented_broker;
use instrumented_broker::InstrumentedBroker;
