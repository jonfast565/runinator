#[cfg(feature = "kafka")]
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

const DEFAULT_CONTROL_TOPIC: &str = "runinator.control";
const DEFAULT_AGENT_TOPIC: &str = "runinator.agent";
const DEFAULT_EFFECT_TOPIC: &str = "runinator.effects";
const DEFAULT_INFRASTRUCTURE_EFFECT_TOPIC: &str = "runinator.effects.infrastructure";
const DEFAULT_EFFECT_RESULT_TOPIC: &str = "runinator.effect-results";
const DEFAULT_WAKE_TOPIC: &str = "runinator.wake";
const DEFAULT_INGRESS_TOPIC: &str = "runinator.ingress";
const DEFAULT_EVENT_TOPIC: &str = "runinator.events";
const DEFAULT_CLIENT_ID: &str = "runinator";

#[cfg(feature = "kafka")]
use parking_lot::Mutex;
#[cfg(feature = "kafka")]
use std::{collections::HashMap, sync::Arc};

#[cfg(feature = "kafka")]
#[derive(Clone, Copy)]
enum KafkaChannel {
    Control,
    Agent,
    Effect,
    InfrastructureEffect,
    EffectResult,
    Wake,
    Ingress,
    Event,
}

#[cfg(feature = "kafka")]
impl KafkaChannel {
    fn topic_for(self, config: &KafkaBrokerConfig) -> &str {
        match self {
            KafkaChannel::Control => &config.control_topic,
            KafkaChannel::Agent => &config.agent_topic,
            KafkaChannel::Effect => &config.effect_topic,
            KafkaChannel::InfrastructureEffect => &config.infrastructure_effect_topic,
            KafkaChannel::EffectResult => &config.effect_result_topic,
            KafkaChannel::Wake => &config.wake_topic,
            KafkaChannel::Ingress => &config.ingress_topic,
            KafkaChannel::Event => &config.event_topic,
        }
    }

    fn name(self) -> &'static str {
        match self {
            KafkaChannel::Control => "control",
            KafkaChannel::Agent => "agent",
            KafkaChannel::Effect => "effects",
            KafkaChannel::InfrastructureEffect => "effects.infrastructure",
            KafkaChannel::EffectResult => "effect-results",
            KafkaChannel::Wake => "wake",
            KafkaChannel::Ingress => "ingress",
            KafkaChannel::Event => "events",
        }
    }

    fn build_consumer(
        self,
        config: &KafkaBrokerConfig,
        consumer_id: &str,
        topic: &str,
    ) -> Result<rdkafka::consumer::StreamConsumer, BrokerError> {
        use rdkafka::{consumer::Consumer, ClientConfig};

        let group_id = match self {
            // directives are competing-consumer and target checked; every agent joins one group so
            // a command is never fanned out to every replica as events are.
            KafkaChannel::Agent => "runinator.agents".to_string(),
            _ => format!("runinator.{consumer_id}.{}", self.name()),
        };
        let client_id = format!("{}.{}.{}", config.client_id, self.name(), consumer_id);
        // events are a fan-out, best-effort stream: a fresh per-replica group starts at the tail so a
        // restarting pod does not replay historical UI events. work channels replay from earliest.
        let offset_reset = match self {
            KafkaChannel::Event => "latest",
            _ => "earliest",
        };
        let consumer: rdkafka::consumer::StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("group.id", group_id)
            .set("client.id", client_id)
            .set("enable.auto.commit", "false")
            .set("enable.auto.offset.store", "false")
            .set("auto.offset.reset", offset_reset)
            .create()
            .map_err(kafka_error("consumer"))?;
        consumer
            .subscribe(&[topic])
            .map_err(kafka_error("subscribe"))?;
        Ok(consumer)
    }
}

#[cfg(feature = "kafka")]
async fn publish_json(
    producer: &rdkafka::producer::FutureProducer,
    topic: &str,
    key: &str,
    payload: String,
) -> Result<(), BrokerError> {
    use rdkafka::{producer::FutureRecord, util::Timeout};
    use std::time::Duration;

    let record = FutureRecord::to(topic).key(key).payload(&payload);
    producer
        .send(record, Timeout::After(Duration::from_secs(10)))
        .await
        .map(|_| ())
        .map_err(|(err, _)| BrokerError::Internal(err.to_string()))
}

#[cfg(feature = "kafka")]
async fn receive_json<T>(
    broker: &KafkaBroker,
    channel: KafkaChannel,
    consumer_id: &str,
) -> Result<(T, PendingMessage), BrokerError>
where
    T: serde::de::DeserializeOwned,
{
    use rdkafka::Message;

    let inner = &broker.inner;
    let consumer = inner.consumer(&broker.config, channel, consumer_id)?;
    let (value, topic, partition, offset) = {
        let message = consumer.recv().await.map_err(kafka_error("receive"))?;
        let payload = message
            .payload()
            .ok_or_else(|| BrokerError::Internal("kafka message had no payload".into()))?;
        let value = serde_json::from_slice(payload)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        (
            value,
            message.topic().to_string(),
            message.partition(),
            message.offset(),
        )
    };

    Ok((
        value,
        PendingMessage {
            consumer,
            topic,
            partition,
            offset,
        },
    ))
}

#[cfg(feature = "kafka")]
fn ack_pending(pending: PendingDelivery) -> Result<(), BrokerError> {
    use rdkafka::{
        consumer::{CommitMode, Consumer},
        topic_partition_list::TopicPartitionList,
        Offset,
    };

    let mut offsets = TopicPartitionList::new();
    offsets
        .add_partition_offset(
            &pending.topic,
            pending.partition,
            Offset::Offset(pending.offset + 1),
        )
        .map_err(kafka_error("ack_offset"))?;
    pending
        .consumer
        .commit(&offsets, CommitMode::Sync)
        .map_err(kafka_error("ack"))
}

#[cfg(feature = "kafka")]
fn nack_pending(pending: PendingDelivery) -> Result<(), BrokerError> {
    use rdkafka::{consumer::Consumer, util::Timeout, Offset};
    use std::time::Duration;

    pending
        .consumer
        .seek(
            &pending.topic,
            pending.partition,
            Offset::Offset(pending.offset),
            Timeout::After(Duration::from_secs(1)),
        )
        .map_err(kafka_error("nack"))
}

#[cfg(feature = "kafka")]
fn kafka_error(context: &'static str) -> impl FnOnce(rdkafka::error::KafkaError) -> BrokerError {
    move |err| BrokerError::Internal(format!("kafka {context}: {err}"))
}

#[cfg(not(feature = "kafka"))]
fn kafka_feature_error() -> BrokerError {
    BrokerError::NotImplemented("kafka broker backend built without kafka feature")
}

#[cfg(test)]
#[path = "kafka_tests.rs"]
mod tests;

mod kafka_broker_config;
pub use kafka_broker_config::KafkaBrokerConfig;

mod kafka_broker;
pub use kafka_broker::KafkaBroker;

#[cfg(feature = "kafka")]
mod kafka_broker_inner;
#[cfg(feature = "kafka")]
use kafka_broker_inner::KafkaBrokerInner;

#[cfg(feature = "kafka")]
mod pending_delivery;
#[cfg(feature = "kafka")]
use pending_delivery::PendingDelivery;

#[cfg(feature = "kafka")]
mod pending_message;
#[cfg(feature = "kafka")]
use pending_message::PendingMessage;
