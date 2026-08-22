//! the broker's concrete transports and external adapters.
//!
//! the [`Broker`] contract, the message/delivery types, and the in-memory backend live in
//! `runinator-broker-core`; this crate adds the backends that need a wire or an external system:
//! the http, tcp, and ws transports, the kafka and rabbitmq adapters, and the [`factory`] that
//! builds one from configuration. it re-exports the core surface at its historical
//! `runinator_broker::…` paths, so a binary that builds a backend needs only this crate.
//!
//! a consumer that merely publishes and receives through a `dyn Broker` should depend on
//! `runinator-broker-core` directly instead.

pub mod adapters;
pub mod dispatch;
mod factory;
#[cfg(feature = "http")]
pub mod http;
pub mod tcp;
pub mod ws;

pub use factory::{
    build_broker_client, build_kafka_broker, build_rabbitmq_broker, BrokerBuildError,
    BrokerClientConfig, BrokerConsumerProfile,
};

// the contract and its backend-independent pieces, re-exported at their historical paths.
pub use runinator_broker_core::{
    ensure_named_workflow_effect_channel, ensure_workflow_effect_channels_supported, in_memory,
    instrument, ActionTarget, AgentCommand, AgentDelivery, AgentDirectiveKind,
    AgentDirectiveResult, AgentDirectiveStatus, Broker, BrokerError, ConnectionState,
    ConsumerProfile, ControlCommand, ControlDelivery, EffectDelivery, EffectExecutor,
    EffectMessage, EffectResult, EffectResultDelivery, EffectResultKind, EffectResultMessage,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, UiEvent, WakeCommand,
    WakeDelivery, WakeMessage, WsIngressCommand, STALE_CONTROL_TTL_SECONDS,
};
