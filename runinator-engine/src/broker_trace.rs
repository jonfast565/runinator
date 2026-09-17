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
use runinator_store::roles::{DeliveryStore, OrchestrationStore};
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

/// Wrap an engine's broker so operator-visible message records are persisted without affecting the
/// live delivery path if the diagnostic write fails.

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

mod tracing_broker;
pub(crate) use tracing_broker::TracingBroker;

mod broker_trace_correlation;
use broker_trace_correlation::BrokerTraceCorrelation;
