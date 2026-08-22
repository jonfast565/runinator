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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaBrokerConfig {
    pub bootstrap_servers: String,
    pub control_topic: String,
    pub agent_topic: String,
    pub effect_topic: String,
    pub infrastructure_effect_topic: String,
    pub effect_result_topic: String,
    pub wake_topic: String,
    pub ingress_topic: String,
    // fan-out: every subscriber uses a distinct group (keyed by consumer id) to read all events.
    pub event_topic: String,
    pub client_id: String,
}

impl KafkaBrokerConfig {
    pub fn new(bootstrap_servers: impl Into<String>) -> Self {
        Self {
            bootstrap_servers: bootstrap_servers.into(),
            control_topic: DEFAULT_CONTROL_TOPIC.into(),
            agent_topic: DEFAULT_AGENT_TOPIC.into(),
            effect_topic: DEFAULT_EFFECT_TOPIC.into(),
            infrastructure_effect_topic: DEFAULT_INFRASTRUCTURE_EFFECT_TOPIC.into(),
            effect_result_topic: DEFAULT_EFFECT_RESULT_TOPIC.into(),
            wake_topic: DEFAULT_WAKE_TOPIC.into(),
            ingress_topic: DEFAULT_INGRESS_TOPIC.into(),
            event_topic: DEFAULT_EVENT_TOPIC.into(),
            client_id: DEFAULT_CLIENT_ID.into(),
        }
    }

    /// override the fan-out topic used for UI events.
    pub fn with_event_topic(mut self, event_topic: impl Into<String>) -> Self {
        self.event_topic = event_topic.into();
        self
    }

    pub fn with_agent_topic(mut self, agent_topic: impl Into<String>) -> Self {
        self.agent_topic = agent_topic.into();
        self
    }

    pub fn with_control_topic(mut self, control_topic: impl Into<String>) -> Self {
        self.control_topic = control_topic.into();
        self
    }

    /// override the orchestration topics (wake = WS -> waker, ingress = waker/worker -> WS).
    pub fn with_orchestration_topics(
        mut self,
        wake_topic: impl Into<String>,
        ingress_topic: impl Into<String>,
    ) -> Self {
        self.wake_topic = wake_topic.into();
        self.ingress_topic = ingress_topic.into();
        self
    }

    pub fn with_effect_topics(
        mut self,
        effect_topic: impl Into<String>,
        infrastructure_effect_topic: impl Into<String>,
        effect_result_topic: impl Into<String>,
    ) -> Self {
        self.effect_topic = effect_topic.into();
        self.infrastructure_effect_topic = infrastructure_effect_topic.into();
        self.effect_result_topic = effect_result_topic.into();
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn has_workflow_effect_topics(&self) -> bool {
        !self.effect_topic.trim().is_empty()
            && !self.infrastructure_effect_topic.trim().is_empty()
            && !self.effect_result_topic.trim().is_empty()
    }
}

pub struct KafkaBroker {
    config: KafkaBrokerConfig,
    #[cfg(feature = "kafka")]
    inner: KafkaBrokerInner,
}

impl KafkaBroker {
    pub fn new(config: KafkaBrokerConfig) -> Result<Self, BrokerError> {
        #[cfg(feature = "kafka")]
        {
            Ok(Self {
                inner: KafkaBrokerInner::new(&config)?,
                config,
            })
        }

        #[cfg(not(feature = "kafka"))]
        {
            Ok(Self { config })
        }
    }

    pub fn config(&self) -> &KafkaBrokerConfig {
        &self.config
    }
}

#[cfg(feature = "kafka")]
struct KafkaBrokerInner {
    producer: rdkafka::producer::FutureProducer,
    control_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    agent_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    effect_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    infrastructure_effect_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    effect_result_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    wake_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    ingress_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    event_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pending: Mutex<HashMap<Uuid, PendingDelivery>>,
}

#[cfg(feature = "kafka")]
use parking_lot::Mutex;
#[cfg(feature = "kafka")]
use std::{collections::HashMap, sync::Arc};

#[cfg(feature = "kafka")]
#[derive(Clone)]
struct PendingDelivery {
    consumer: Arc<rdkafka::consumer::StreamConsumer>,
    topic: String,
    partition: i32,
    offset: i64,
}

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
impl KafkaBrokerInner {
    fn new(config: &KafkaBrokerConfig) -> Result<Self, BrokerError> {
        use rdkafka::ClientConfig;

        let producer = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("client.id", &config.client_id)
            .create()
            .map_err(kafka_error("producer"))?;

        Ok(Self {
            producer,
            control_consumers: Mutex::new(HashMap::new()),
            agent_consumers: Mutex::new(HashMap::new()),
            effect_consumers: Mutex::new(HashMap::new()),
            infrastructure_effect_consumers: Mutex::new(HashMap::new()),
            effect_result_consumers: Mutex::new(HashMap::new()),
            wake_consumers: Mutex::new(HashMap::new()),
            ingress_consumers: Mutex::new(HashMap::new()),
            event_consumers: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        })
    }

    fn consumer(
        &self,
        config: &KafkaBrokerConfig,
        channel: KafkaChannel,
        consumer_id: &str,
    ) -> Result<Arc<rdkafka::consumer::StreamConsumer>, BrokerError> {
        let map = match channel {
            KafkaChannel::Control => &self.control_consumers,
            KafkaChannel::Agent => &self.agent_consumers,
            KafkaChannel::Effect => &self.effect_consumers,
            KafkaChannel::InfrastructureEffect => &self.infrastructure_effect_consumers,
            KafkaChannel::EffectResult => &self.effect_result_consumers,
            KafkaChannel::Wake => &self.wake_consumers,
            KafkaChannel::Ingress => &self.ingress_consumers,
            KafkaChannel::Event => &self.event_consumers,
        };

        if let Some(consumer) = map.lock().get(consumer_id).cloned() {
            return Ok(consumer);
        }

        let topic = channel.topic_for(config);
        let consumer = Arc::new(channel.build_consumer(config, consumer_id, topic)?);
        map.lock()
            .insert(consumer_id.to_string(), Arc::clone(&consumer));
        Ok(consumer)
    }

    fn track_delivery(
        &self,
        delivery_id: Uuid,
        consumer: Arc<rdkafka::consumer::StreamConsumer>,
        topic: String,
        partition: i32,
        offset: i64,
    ) {
        self.pending.lock().insert(
            delivery_id,
            PendingDelivery {
                consumer,
                topic,
                partition,
                offset,
            },
        );
    }

    fn take_pending(&self, delivery_id: Uuid) -> Result<PendingDelivery, BrokerError> {
        self.pending
            .lock()
            .remove(&delivery_id)
            .ok_or(BrokerError::UnknownDelivery(delivery_id))
    }
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
struct PendingMessage {
    consumer: Arc<rdkafka::consumer::StreamConsumer>,
    topic: String,
    partition: i32,
    offset: i64,
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

#[cfg(feature = "kafka")]
impl KafkaBroker {
    async fn receive_effect_from(
        &self,
        channel: KafkaChannel,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        let (message, pending) = receive_json::<EffectMessage>(self, channel, consumer).await?;
        let delivery = EffectDelivery::from(message);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }
}

#[async_trait]
#[cfg(feature = "kafka")]
impl Broker for KafkaBroker {
    fn supports_agent_channel(&self) -> bool {
        !self.config.agent_topic.trim().is_empty()
    }

    fn supports_workflow_effect_channels(&self) -> bool {
        self.config.has_workflow_effect_topics()
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let key = command.workflow_run_id.to_string();
        let payload = serde_json::to_string(&command)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.producer,
            &self.config.control_topic,
            &key,
            payload,
        )
        .await
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        let (command, pending) =
            receive_json::<ControlCommand>(self, KafkaChannel::Control, consumer).await?;
        let delivery = ControlDelivery::from(command);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }

    async fn ack_control(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack_control(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let key = command.replica_id.to_string();
        let payload = serde_json::to_string(&command)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.producer,
            &self.config.agent_topic,
            &key,
            payload,
        )
        .await
    }

    async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
        let (command, pending) =
            receive_json::<AgentCommand>(self, KafkaChannel::Agent, consumer).await?;
        let delivery = AgentDelivery::from(command);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }

    async fn ack_agent(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack_agent(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let topic = match message.command.executor {
            EffectExecutor::Provider => &self.config.effect_topic,
            EffectExecutor::Infrastructure => &self.config.infrastructure_effect_topic,
        };
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(&self.inner.producer, topic, &key, payload).await
    }

    async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
        self.receive_effect_from(KafkaChannel::Effect, consumer)
            .await
    }

    async fn receive_infrastructure_effect(
        &self,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        self.receive_effect_from(KafkaChannel::InfrastructureEffect, consumer)
            .await
    }

    async fn ack_effect(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack_effect(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.producer,
            &self.config.effect_result_topic,
            &key,
            payload,
        )
        .await
    }

    async fn receive_effect_result(
        &self,
        consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        let (message, pending) =
            receive_json::<EffectResultMessage>(self, KafkaChannel::EffectResult, consumer).await?;
        let delivery = EffectResultDelivery::from(message);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }

    async fn ack_effect_result(
        &self,
        _consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack_effect_result(
        &self,
        _consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        nack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(&self.inner.producer, &self.config.wake_topic, &key, payload).await
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        let (message, pending) =
            receive_json::<WakeMessage>(self, KafkaChannel::Wake, consumer).await?;
        let delivery = WakeDelivery::from(message);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }

    async fn ack_wake(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack_wake(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.producer,
            &self.config.ingress_topic,
            &key,
            payload,
        )
        .await
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        let (message, pending) =
            receive_json::<IngressMessage>(self, KafkaChannel::Ingress, consumer).await?;
        let delivery = IngressDelivery::from(message);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }

    async fn ack_ingress(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack_ingress(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        // empty key: events are not partitioned by entity, fan-out reads every partition.
        publish_json(&self.inner.producer, &self.config.event_topic, "", payload).await
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        // each subscriber's unique group reads every partition; best-effort, so no offset commit.
        let (message, _pending) =
            receive_json::<EventMessage>(self, KafkaChannel::Event, consumer).await?;
        Ok(EventDelivery::from(message))
    }
}

#[async_trait]
#[cfg(not(feature = "kafka"))]
impl Broker for KafkaBroker {
    async fn publish_control(&self, _command: ControlCommand) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        Err(kafka_feature_error())
    }

    async fn ack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn nack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn publish_wake(&self, _message: WakeMessage) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn receive_wake(&self, _consumer: &str) -> Result<WakeDelivery, BrokerError> {
        Err(kafka_feature_error())
    }

    async fn ack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn nack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn publish_ingress(&self, _message: IngressMessage) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn receive_ingress(&self, _consumer: &str) -> Result<IngressDelivery, BrokerError> {
        Err(kafka_feature_error())
    }

    async fn ack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn nack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn publish_event(&self, _message: EventMessage) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn receive_event(&self, _consumer: &str) -> Result<EventDelivery, BrokerError> {
        Err(kafka_feature_error())
    }
}

#[cfg(not(feature = "kafka"))]
fn kafka_feature_error() -> BrokerError {
    BrokerError::NotImplemented("kafka broker backend built without kafka feature")
}

#[cfg(test)]
#[path = "kafka_tests.rs"]
mod tests;
