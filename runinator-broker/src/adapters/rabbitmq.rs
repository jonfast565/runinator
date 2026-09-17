#[cfg(feature = "rabbitmq")]
use crate::ConsumerProfile;
#[cfg(feature = "rabbitmq")]
use crate::{
    AgentCommand, AgentDelivery, EffectDelivery, EffectExecutor, EffectMessage,
    EffectResultDelivery, EffectResultMessage,
};
use crate::{
    Broker, BrokerError, ControlCommand, ControlDelivery, EventDelivery, EventMessage,
    IngressDelivery, IngressMessage, WakeDelivery, WakeMessage,
};
use async_trait::async_trait;
use uuid::Uuid;

const DEFAULT_CONTROL_QUEUE: &str = "runinator.control";
const DEFAULT_AGENT_QUEUE_PREFIX: &str = "runinator.agent";
const DEFAULT_EFFECT_QUEUE: &str = "runinator.effects";
const DEFAULT_INFRASTRUCTURE_EFFECT_QUEUE: &str = "runinator.effects.infrastructure";
const DEFAULT_EFFECT_RESULT_QUEUE: &str = "runinator.effect-results";
const DEFAULT_WAKE_QUEUE: &str = "runinator.wake";
const DEFAULT_INGRESS_QUEUE: &str = "runinator.ingress";
const DEFAULT_EVENT_EXCHANGE: &str = "runinator.events";
const DEFAULT_CLIENT_ID: &str = "runinator";
// bounds unacked deliveries per consumer so a queue backlog is spread across consumers instead of
// being pushed wholesale to whichever subscribed first.
const DEFAULT_PREFETCH_COUNT: u16 = 64;

#[cfg(feature = "rabbitmq")]
use futures_util::StreamExt;
#[cfg(feature = "rabbitmq")]
use log::{info, warn};
#[cfg(feature = "rabbitmq")]
use parking_lot::Mutex;
#[cfg(feature = "rabbitmq")]
use std::{collections::HashMap, sync::Arc};
#[cfg(feature = "rabbitmq")]
use tokio::sync::Mutex as AsyncMutex;

#[cfg(feature = "rabbitmq")]
#[derive(Clone, Copy)]
enum RabbitMqChannel {
    Control,
    Effect,
    InfrastructureEffect,
    EffectResult,
    Wake,
    Ingress,
}

#[cfg(feature = "rabbitmq")]
async fn receive_json<T>(
    broker: &RabbitMqBroker,
    channel: RabbitMqChannel,
    consumer_id: &str,
) -> Result<(T, lapin::message::Delivery), BrokerError>
where
    T: serde::de::DeserializeOwned,
{
    let consumer = broker
        .inner
        .consumer(&broker.config, channel, consumer_id)
        .await?;
    let mut guard = consumer.lock().await;
    let delivery = guard
        .next()
        .await
        .ok_or(BrokerError::ConsumerStreamEnded)?
        .map_err(rabbitmq_error("receive"))?;
    let value = serde_json::from_slice(&delivery.data)
        .map_err(|err| BrokerError::Internal(err.to_string()))?;
    Ok((value, delivery))
}

#[cfg(feature = "rabbitmq")]
async fn ack_delivery(delivery: lapin::message::Delivery) -> Result<(), BrokerError> {
    delivery
        .ack(lapin::options::BasicAckOptions::default())
        .await
        .map(|_| ())
        .map_err(rabbitmq_error("ack"))
}

#[cfg(feature = "rabbitmq")]
async fn nack_delivery(delivery: lapin::message::Delivery) -> Result<(), BrokerError> {
    delivery
        .nack(lapin::options::BasicNackOptions {
            requeue: true,
            ..Default::default()
        })
        .await
        .map(|_| ())
        .map_err(rabbitmq_error("nack"))
}

#[cfg(feature = "rabbitmq")]
fn queue_for(config: &RabbitMqBrokerConfig, channel: RabbitMqChannel) -> &str {
    match channel {
        RabbitMqChannel::Control => &config.control_queue,
        RabbitMqChannel::Effect => &config.effect_queue,
        RabbitMqChannel::InfrastructureEffect => &config.infrastructure_effect_queue,
        RabbitMqChannel::EffectResult => &config.effect_result_queue,
        RabbitMqChannel::Wake => &config.wake_queue,
        RabbitMqChannel::Ingress => &config.ingress_queue,
    }
}

#[cfg(feature = "rabbitmq")]
fn agent_queue(config: &RabbitMqBrokerConfig, replica_id: Uuid) -> String {
    format!("{}.{}", config.agent_queue_prefix, replica_id)
}

#[cfg(feature = "rabbitmq")]
async fn receive_agent_json(
    broker: &RabbitMqBroker,
    profile: &ConsumerProfile,
) -> Result<(AgentCommand, lapin::message::Delivery), BrokerError> {
    let replica_id = profile
        .replica_id
        .ok_or_else(|| BrokerError::Internal("agent receive requires a replica id".into()))?;
    let consumer = broker
        .inner
        .agent_consumer(&broker.config, replica_id, &profile.id)
        .await?;
    let mut guard = consumer.lock().await;
    let delivery = guard
        .next()
        .await
        .ok_or(BrokerError::ConsumerStreamEnded)?
        .map_err(rabbitmq_error("agent_receive"))?;
    let command = serde_json::from_slice(&delivery.data)
        .map_err(|err| BrokerError::Internal(err.to_string()))?;
    Ok((command, delivery))
}

#[cfg(feature = "rabbitmq")]
fn channel_name(channel: RabbitMqChannel) -> &'static str {
    match channel {
        RabbitMqChannel::Control => "control",
        RabbitMqChannel::Effect => "effects",
        RabbitMqChannel::InfrastructureEffect => "effects.infrastructure",
        RabbitMqChannel::EffectResult => "effect-results",
        RabbitMqChannel::Wake => "wake",
        RabbitMqChannel::Ingress => "ingress",
    }
}

#[cfg(feature = "rabbitmq")]
fn rabbitmq_error(context: &'static str) -> impl FnOnce(lapin::Error) -> BrokerError {
    move |err| BrokerError::Internal(format!("rabbitmq {context}: {err}"))
}

#[cfg(not(feature = "rabbitmq"))]
fn rabbitmq_feature_error() -> BrokerError {
    BrokerError::NotImplemented("rabbitmq broker backend built without rabbitmq feature")
}

#[cfg(test)]
#[path = "rabbitmq_tests.rs"]
mod tests;

mod rabbit_mq_broker_config;
pub use rabbit_mq_broker_config::RabbitMqBrokerConfig;

mod rabbit_mq_broker;
pub use rabbit_mq_broker::RabbitMqBroker;

#[cfg(feature = "rabbitmq")]
mod rabbit_mq_broker_inner;
#[cfg(feature = "rabbitmq")]
use rabbit_mq_broker_inner::RabbitMqBrokerInner;

#[cfg(feature = "rabbitmq")]
mod rabbit_channel;
#[cfg(feature = "rabbitmq")]
use rabbit_channel::RabbitChannel;
