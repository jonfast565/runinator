use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, ResultDelivery, ResultMessage,
    WakeDelivery, WakeMessage,
};
use async_trait::async_trait;
use uuid::Uuid;

const DEFAULT_ACTION_TOPIC: &str = "runinator.actions";
const DEFAULT_CONTROL_TOPIC: &str = "runinator.control";
const DEFAULT_RESULT_TOPIC: &str = "runinator.results";
const DEFAULT_WAKE_TOPIC: &str = "runinator.wake";
const DEFAULT_INGRESS_TOPIC: &str = "runinator.ingress";
const DEFAULT_EVENT_TOPIC: &str = "runinator.events";
const DEFAULT_CLIENT_ID: &str = "runinator";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaBrokerConfig {
    pub bootstrap_servers: String,
    pub action_topic: String,
    pub control_topic: String,
    pub result_topic: String,
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
            action_topic: DEFAULT_ACTION_TOPIC.into(),
            control_topic: DEFAULT_CONTROL_TOPIC.into(),
            result_topic: DEFAULT_RESULT_TOPIC.into(),
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

    pub fn with_topics(
        mut self,
        action_topic: impl Into<String>,
        control_topic: impl Into<String>,
        result_topic: impl Into<String>,
    ) -> Self {
        self.action_topic = action_topic.into();
        self.control_topic = control_topic.into();
        self.result_topic = result_topic.into();
        self
    }

    /// override the orchestration topics (wake = ws -> waker, ingress = waker/worker -> ws).
    pub fn with_orchestration_topics(
        mut self,
        wake_topic: impl Into<String>,
        ingress_topic: impl Into<String>,
    ) -> Self {
        self.wake_topic = wake_topic.into();
        self.ingress_topic = ingress_topic.into();
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn has_workflow_result_topic(&self) -> bool {
        !self.result_topic.trim().is_empty()
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
    action_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    control_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    result_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
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
    Action,
    Control,
    Result,
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
            action_consumers: Mutex::new(HashMap::new()),
            control_consumers: Mutex::new(HashMap::new()),
            result_consumers: Mutex::new(HashMap::new()),
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
            KafkaChannel::Action => &self.action_consumers,
            KafkaChannel::Control => &self.control_consumers,
            KafkaChannel::Result => &self.result_consumers,
            KafkaChannel::Wake => &self.wake_consumers,
            KafkaChannel::Ingress => &self.ingress_consumers,
            KafkaChannel::Event => &self.event_consumers,
        };

        if let Some(consumer) = map.lock().get(consumer_id).cloned() {
            return Ok(consumer);
        }

        let topic = topic_for(config, channel);
        let consumer = Arc::new(build_consumer(config, channel, consumer_id, topic)?);
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
fn build_consumer(
    config: &KafkaBrokerConfig,
    channel: KafkaChannel,
    consumer_id: &str,
    topic: &str,
) -> Result<rdkafka::consumer::StreamConsumer, BrokerError> {
    use rdkafka::{consumer::Consumer, ClientConfig};

    let group_id = format!("runinator.{consumer_id}.{}", channel_name(channel));
    let client_id = format!(
        "{}.{}.{}",
        config.client_id,
        channel_name(channel),
        consumer_id
    );
    // events are a fan-out, best-effort stream: a fresh per-replica group starts at the tail so a
    // restarting pod does not replay historical UI events. work channels replay from earliest.
    let offset_reset = match channel {
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
fn topic_for(config: &KafkaBrokerConfig, channel: KafkaChannel) -> &str {
    match channel {
        KafkaChannel::Action => &config.action_topic,
        KafkaChannel::Control => &config.control_topic,
        KafkaChannel::Result => &config.result_topic,
        KafkaChannel::Wake => &config.wake_topic,
        KafkaChannel::Ingress => &config.ingress_topic,
        KafkaChannel::Event => &config.event_topic,
    }
}

#[cfg(feature = "kafka")]
fn channel_name(channel: KafkaChannel) -> &'static str {
    match channel {
        KafkaChannel::Action => "actions",
        KafkaChannel::Control => "control",
        KafkaChannel::Result => "results",
        KafkaChannel::Wake => "wake",
        KafkaChannel::Ingress => "ingress",
        KafkaChannel::Event => "events",
    }
}

#[cfg(feature = "kafka")]
fn kafka_error(context: &'static str) -> impl FnOnce(rdkafka::error::KafkaError) -> BrokerError {
    move |err| BrokerError::Internal(format!("kafka {context}: {err}"))
}

#[async_trait]
#[cfg(feature = "kafka")]
impl Broker for KafkaBroker {
    fn supports_workflow_result_channels(&self) -> bool {
        self.config.has_workflow_result_topic()
    }

    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.producer,
            &self.config.action_topic,
            &key,
            payload,
        )
        .await
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        let (message, pending) =
            receive_json::<BrokerMessage>(self, KafkaChannel::Action, consumer).await?;
        let delivery = BrokerDelivery::from(message);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }

    async fn ack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_pending(self.inner.take_pending(delivery_id)?)
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

    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.producer,
            &self.config.result_topic,
            &key,
            payload,
        )
        .await
    }

    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
        let (message, pending) =
            receive_json::<ResultMessage>(self, KafkaChannel::Result, consumer).await?;
        let delivery = ResultDelivery::from(message);
        self.inner.track_delivery(
            delivery.delivery_id,
            pending.consumer,
            pending.topic,
            pending.partition,
            pending.offset,
        );
        Ok(delivery)
    }

    async fn ack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_pending(self.inner.take_pending(delivery_id)?)
    }

    async fn nack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
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
    fn supports_workflow_result_channels(&self) -> bool {
        false
    }

    async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        Err(kafka_feature_error())
    }

    async fn ack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn nack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn publish_control(&self, _command: ControlCommand) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        Err(kafka_feature_error())
    }

    async fn ack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn publish_result(&self, _message: ResultMessage) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn receive_result(&self, _consumer: &str) -> Result<ResultDelivery, BrokerError> {
        Err(kafka_feature_error())
    }

    async fn ack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(kafka_feature_error())
    }

    async fn nack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
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
mod tests {
    use super::*;

    #[test]
    fn kafka_config_defaults_topics_and_client_id() {
        let config = KafkaBrokerConfig::new("localhost:9092");

        assert_eq!(config.bootstrap_servers, "localhost:9092");
        assert_eq!(config.action_topic, DEFAULT_ACTION_TOPIC);
        assert_eq!(config.control_topic, DEFAULT_CONTROL_TOPIC);
        assert_eq!(config.result_topic, DEFAULT_RESULT_TOPIC);
        assert_eq!(config.client_id, DEFAULT_CLIENT_ID);
    }

    #[test]
    fn kafka_config_accepts_topic_and_client_overrides() {
        let config = KafkaBrokerConfig::new("localhost:9092")
            .with_topics("a", "c", "r")
            .with_client_id("test-client");

        assert_eq!(config.action_topic, "a");
        assert_eq!(config.control_topic, "c");
        assert_eq!(config.result_topic, "r");
        assert_eq!(config.client_id, "test-client");
    }

    #[test]
    fn kafka_config_detects_missing_result_topic() {
        let config = KafkaBrokerConfig::new("localhost:9092").with_topics("a", "c", " ");

        assert!(!config.has_workflow_result_topic());
    }
}
