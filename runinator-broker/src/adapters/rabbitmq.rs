use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, ResultDelivery, ResultMessage,
    WakeDelivery, WakeMessage,
};
use async_trait::async_trait;
use uuid::Uuid;

const DEFAULT_ACTION_QUEUE: &str = "runinator.actions";
const DEFAULT_CONTROL_QUEUE: &str = "runinator.control";
const DEFAULT_RESULT_QUEUE: &str = "runinator.results";
const DEFAULT_WAKE_QUEUE: &str = "runinator.wake";
const DEFAULT_INGRESS_QUEUE: &str = "runinator.ingress";
const DEFAULT_EVENT_EXCHANGE: &str = "runinator.events";
const DEFAULT_CLIENT_ID: &str = "runinator";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RabbitMqBrokerConfig {
    pub uri: String,
    pub action_queue: String,
    pub control_queue: String,
    pub result_queue: String,
    pub wake_queue: String,
    pub ingress_queue: String,
    // fan-out exchange for UI events; each subscriber binds its own exclusive queue.
    pub event_exchange: String,
    pub client_id: String,
}

impl RabbitMqBrokerConfig {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            action_queue: DEFAULT_ACTION_QUEUE.into(),
            control_queue: DEFAULT_CONTROL_QUEUE.into(),
            result_queue: DEFAULT_RESULT_QUEUE.into(),
            wake_queue: DEFAULT_WAKE_QUEUE.into(),
            ingress_queue: DEFAULT_INGRESS_QUEUE.into(),
            event_exchange: DEFAULT_EVENT_EXCHANGE.into(),
            client_id: DEFAULT_CLIENT_ID.into(),
        }
    }

    /// override the fan-out exchange used for UI events.
    pub fn with_event_exchange(mut self, event_exchange: impl Into<String>) -> Self {
        self.event_exchange = event_exchange.into();
        self
    }

    pub fn with_queues(
        mut self,
        action_queue: impl Into<String>,
        control_queue: impl Into<String>,
        result_queue: impl Into<String>,
    ) -> Self {
        self.action_queue = action_queue.into();
        self.control_queue = control_queue.into();
        self.result_queue = result_queue.into();
        self
    }

    /// override the orchestration queues (wake = ws -> waker, ingress = waker/worker -> ws).
    pub fn with_orchestration_queues(
        mut self,
        wake_queue: impl Into<String>,
        ingress_queue: impl Into<String>,
    ) -> Self {
        self.wake_queue = wake_queue.into();
        self.ingress_queue = ingress_queue.into();
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
    }

    pub fn has_workflow_result_queue(&self) -> bool {
        !self.result_queue.trim().is_empty()
    }
}

pub struct RabbitMqBroker {
    config: RabbitMqBrokerConfig,
    #[cfg(feature = "rabbitmq")]
    inner: RabbitMqBrokerInner,
}

impl RabbitMqBroker {
    pub async fn connect(config: RabbitMqBrokerConfig) -> Result<Self, BrokerError> {
        #[cfg(feature = "rabbitmq")]
        {
            Ok(Self {
                inner: RabbitMqBrokerInner::connect(&config).await?,
                config,
            })
        }

        #[cfg(not(feature = "rabbitmq"))]
        {
            Ok(Self { config })
        }
    }

    pub fn config(&self) -> &RabbitMqBrokerConfig {
        &self.config
    }
}

#[cfg(feature = "rabbitmq")]
use futures_util::StreamExt;
#[cfg(feature = "rabbitmq")]
use parking_lot::Mutex;
#[cfg(feature = "rabbitmq")]
use std::{collections::HashMap, sync::Arc};
#[cfg(feature = "rabbitmq")]
use tokio::sync::Mutex as AsyncMutex;

#[cfg(feature = "rabbitmq")]
struct RabbitMqBrokerInner {
    channel: lapin::Channel,
    action_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    control_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    result_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    wake_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    ingress_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    // each subscriber gets its own exclusive auto-delete queue bound to the fan-out exchange.
    event_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pending: Mutex<HashMap<Uuid, lapin::message::Delivery>>,
}

#[cfg(feature = "rabbitmq")]
#[derive(Clone, Copy)]
enum RabbitMqChannel {
    Action,
    Control,
    Result,
    Wake,
    Ingress,
}

#[cfg(feature = "rabbitmq")]
impl RabbitMqBrokerInner {
    async fn connect(config: &RabbitMqBrokerConfig) -> Result<Self, BrokerError> {
        use lapin::{Connection, ConnectionProperties};

        let connection = Connection::connect(&config.uri, ConnectionProperties::default())
            .await
            .map_err(rabbitmq_error("connect"))?;
        let channel = connection
            .create_channel()
            .await
            .map_err(rabbitmq_error("channel"))?;
        declare_queue(&channel, &config.action_queue).await?;
        declare_queue(&channel, &config.control_queue).await?;
        declare_queue(&channel, &config.result_queue).await?;
        declare_queue(&channel, &config.wake_queue).await?;
        declare_queue(&channel, &config.ingress_queue).await?;
        declare_fanout_exchange(&channel, &config.event_exchange).await?;

        Ok(Self {
            channel,
            action_consumers: Mutex::new(HashMap::new()),
            control_consumers: Mutex::new(HashMap::new()),
            result_consumers: Mutex::new(HashMap::new()),
            wake_consumers: Mutex::new(HashMap::new()),
            ingress_consumers: Mutex::new(HashMap::new()),
            event_consumers: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        })
    }

    /// get-or-create one subscriber's exclusive queue bound to the fan-out events exchange.
    async fn event_consumer(
        &self,
        config: &RabbitMqBrokerConfig,
        consumer_id: &str,
    ) -> Result<Arc<AsyncMutex<lapin::Consumer>>, BrokerError> {
        if let Some(consumer) = self.event_consumers.lock().get(consumer_id).cloned() {
            return Ok(consumer);
        }

        // a per-subscriber exclusive, auto-delete queue bound to the fanout exchange gives this
        // replica its own copy of every event; auto-ack since UI events are best-effort.
        let queue_name = format!("{}.events.{}", config.client_id, consumer_id);
        self.channel
            .queue_declare(
                queue_name.as_str().into(),
                lapin::options::QueueDeclareOptions {
                    durable: false,
                    exclusive: true,
                    auto_delete: true,
                    ..Default::default()
                },
                lapin::types::FieldTable::default(),
            )
            .await
            .map_err(rabbitmq_error("event_queue_declare"))?;
        self.channel
            .queue_bind(
                queue_name.as_str().into(),
                config.event_exchange.as_str().into(),
                "".into(),
                lapin::options::QueueBindOptions::default(),
                lapin::types::FieldTable::default(),
            )
            .await
            .map_err(rabbitmq_error("event_queue_bind"))?;
        let tag = format!("{}.events.{}", config.client_id, consumer_id);
        let consumer = Arc::new(AsyncMutex::new(
            self.channel
                .basic_consume(
                    queue_name.as_str().into(),
                    tag.into(),
                    lapin::options::BasicConsumeOptions {
                        no_ack: true,
                        ..Default::default()
                    },
                    lapin::types::FieldTable::default(),
                )
                .await
                .map_err(rabbitmq_error("event_consume"))?,
        ));
        self.event_consumers
            .lock()
            .insert(consumer_id.to_string(), Arc::clone(&consumer));
        Ok(consumer)
    }

    async fn consumer(
        &self,
        config: &RabbitMqBrokerConfig,
        channel: RabbitMqChannel,
        consumer_id: &str,
    ) -> Result<Arc<AsyncMutex<lapin::Consumer>>, BrokerError> {
        let map = match channel {
            RabbitMqChannel::Action => &self.action_consumers,
            RabbitMqChannel::Control => &self.control_consumers,
            RabbitMqChannel::Result => &self.result_consumers,
            RabbitMqChannel::Wake => &self.wake_consumers,
            RabbitMqChannel::Ingress => &self.ingress_consumers,
        };

        if let Some(consumer) = map.lock().get(consumer_id).cloned() {
            return Ok(consumer);
        }

        let queue = queue_for(config, channel);
        let tag = format!(
            "{}.{}.{}",
            config.client_id,
            channel_name(channel),
            consumer_id
        );
        let consumer = Arc::new(AsyncMutex::new(
            self.channel
                .basic_consume(
                    queue.into(),
                    tag.into(),
                    lapin::options::BasicConsumeOptions::default(),
                    lapin::types::FieldTable::default(),
                )
                .await
                .map_err(rabbitmq_error("consume"))?,
        ));
        map.lock()
            .insert(consumer_id.to_string(), Arc::clone(&consumer));
        Ok(consumer)
    }

    fn track_delivery(&self, delivery_id: Uuid, delivery: lapin::message::Delivery) {
        self.pending.lock().insert(delivery_id, delivery);
    }

    fn take_pending(&self, delivery_id: Uuid) -> Result<lapin::message::Delivery, BrokerError> {
        self.pending
            .lock()
            .remove(&delivery_id)
            .ok_or(BrokerError::UnknownDelivery(delivery_id))
    }
}

#[cfg(feature = "rabbitmq")]
async fn declare_queue(channel: &lapin::Channel, queue: &str) -> Result<(), BrokerError> {
    channel
        .queue_declare(
            queue.into(),
            lapin::options::QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            lapin::types::FieldTable::default(),
        )
        .await
        .map(|_| ())
        .map_err(rabbitmq_error("queue_declare"))
}

#[cfg(feature = "rabbitmq")]
async fn declare_fanout_exchange(
    channel: &lapin::Channel,
    exchange: &str,
) -> Result<(), BrokerError> {
    channel
        .exchange_declare(
            exchange.into(),
            lapin::ExchangeKind::Fanout,
            lapin::options::ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            lapin::types::FieldTable::default(),
        )
        .await
        .map(|_| ())
        .map_err(rabbitmq_error("exchange_declare"))
}

#[cfg(feature = "rabbitmq")]
async fn publish_fanout(
    channel: &lapin::Channel,
    exchange: &str,
    payload: String,
) -> Result<(), BrokerError> {
    channel
        .basic_publish(
            exchange.into(),
            "".into(),
            lapin::options::BasicPublishOptions::default(),
            payload.as_bytes(),
            lapin::BasicProperties::default(),
        )
        .await
        .map_err(rabbitmq_error("publish_event"))?
        .await
        .map_err(rabbitmq_error("publish_event_confirm"))?;
    Ok(())
}

#[cfg(feature = "rabbitmq")]
async fn publish_json(
    channel: &lapin::Channel,
    queue: &str,
    key: &str,
    payload: String,
) -> Result<(), BrokerError> {
    channel
        .basic_publish(
            "".into(),
            queue.into(),
            lapin::options::BasicPublishOptions::default(),
            payload.as_bytes(),
            lapin::BasicProperties::default()
                .with_delivery_mode(2)
                .with_message_id(key.into()),
        )
        .await
        .map_err(rabbitmq_error("publish"))?
        .await
        .map_err(rabbitmq_error("publish_confirm"))?;
    Ok(())
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
        .ok_or_else(|| BrokerError::Internal("rabbitmq consumer stream ended".into()))?
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
        RabbitMqChannel::Action => &config.action_queue,
        RabbitMqChannel::Control => &config.control_queue,
        RabbitMqChannel::Result => &config.result_queue,
        RabbitMqChannel::Wake => &config.wake_queue,
        RabbitMqChannel::Ingress => &config.ingress_queue,
    }
}

#[cfg(feature = "rabbitmq")]
fn channel_name(channel: RabbitMqChannel) -> &'static str {
    match channel {
        RabbitMqChannel::Action => "actions",
        RabbitMqChannel::Control => "control",
        RabbitMqChannel::Result => "results",
        RabbitMqChannel::Wake => "wake",
        RabbitMqChannel::Ingress => "ingress",
    }
}

#[cfg(feature = "rabbitmq")]
fn rabbitmq_error(context: &'static str) -> impl FnOnce(lapin::Error) -> BrokerError {
    move |err| BrokerError::Internal(format!("rabbitmq {context}: {err}"))
}

#[async_trait]
#[cfg(feature = "rabbitmq")]
impl Broker for RabbitMqBroker {
    fn supports_workflow_result_channels(&self) -> bool {
        self.config.has_workflow_result_queue()
    }

    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.channel,
            &self.config.action_queue,
            &key,
            payload,
        )
        .await
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        let (message, delivery) =
            receive_json::<BrokerMessage>(self, RabbitMqChannel::Action, consumer).await?;
        let broker_delivery = BrokerDelivery::from(message);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    async fn ack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn nack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let key = command.workflow_run_id.to_string();
        let payload = serde_json::to_string(&command)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.channel,
            &self.config.control_queue,
            &key,
            payload,
        )
        .await
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        let (command, delivery) =
            receive_json::<ControlCommand>(self, RabbitMqChannel::Control, consumer).await?;
        let broker_delivery = ControlDelivery::from(command);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    async fn ack_control(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.channel,
            &self.config.result_queue,
            &key,
            payload,
        )
        .await
    }

    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
        let (message, delivery) =
            receive_json::<ResultMessage>(self, RabbitMqChannel::Result, consumer).await?;
        let broker_delivery = ResultDelivery::from(message);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    async fn ack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn nack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(&self.inner.channel, &self.config.wake_queue, &key, payload).await
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        let (message, delivery) =
            receive_json::<WakeMessage>(self, RabbitMqChannel::Wake, consumer).await?;
        let broker_delivery = WakeDelivery::from(message);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    async fn ack_wake(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn nack_wake(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_json(
            &self.inner.channel,
            &self.config.ingress_queue,
            &key,
            payload,
        )
        .await
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        let (message, delivery) =
            receive_json::<IngressMessage>(self, RabbitMqChannel::Ingress, consumer).await?;
        let broker_delivery = IngressDelivery::from(message);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    async fn ack_ingress(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn nack_ingress(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        publish_fanout(&self.inner.channel, &self.config.event_exchange, payload).await
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        let subscriber = self.inner.event_consumer(&self.config, consumer).await?;
        let mut guard = subscriber.lock().await;
        let delivery = guard
            .next()
            .await
            .ok_or_else(|| BrokerError::Internal("rabbitmq event stream ended".into()))?
            .map_err(rabbitmq_error("receive_event"))?;
        let message: EventMessage = serde_json::from_slice(&delivery.data)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        // auto-ack consumer: nothing to track.
        Ok(EventDelivery::from(message))
    }
}

#[async_trait]
#[cfg(not(feature = "rabbitmq"))]
impl Broker for RabbitMqBroker {
    fn supports_workflow_result_channels(&self) -> bool {
        false
    }

    async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn ack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn nack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn publish_control(&self, _command: ControlCommand) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn ack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn publish_result(&self, _message: ResultMessage) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn receive_result(&self, _consumer: &str) -> Result<ResultDelivery, BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn ack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn nack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn publish_wake(&self, _message: WakeMessage) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn receive_wake(&self, _consumer: &str) -> Result<WakeDelivery, BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn ack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn nack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn publish_ingress(&self, _message: IngressMessage) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn receive_ingress(&self, _consumer: &str) -> Result<IngressDelivery, BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn ack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn nack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn publish_event(&self, _message: EventMessage) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn receive_event(&self, _consumer: &str) -> Result<EventDelivery, BrokerError> {
        Err(rabbitmq_feature_error())
    }
}

#[cfg(not(feature = "rabbitmq"))]
fn rabbitmq_feature_error() -> BrokerError {
    BrokerError::NotImplemented("rabbitmq broker backend built without rabbitmq feature")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rabbitmq_config_defaults_queues_and_client_id() {
        let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f");

        assert_eq!(config.uri, "amqp://127.0.0.1:5672/%2f");
        assert_eq!(config.action_queue, DEFAULT_ACTION_QUEUE);
        assert_eq!(config.control_queue, DEFAULT_CONTROL_QUEUE);
        assert_eq!(config.result_queue, DEFAULT_RESULT_QUEUE);
        assert_eq!(config.client_id, DEFAULT_CLIENT_ID);
    }

    #[test]
    fn rabbitmq_config_accepts_queue_and_client_overrides() {
        let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f")
            .with_queues("a", "c", "r")
            .with_client_id("test-client");

        assert_eq!(config.action_queue, "a");
        assert_eq!(config.control_queue, "c");
        assert_eq!(config.result_queue, "r");
        assert_eq!(config.client_id, "test-client");
    }

    #[test]
    fn rabbitmq_config_detects_missing_result_queue() {
        let config =
            RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f").with_queues("a", "c", " ");

        assert!(!config.has_workflow_result_queue());
    }
}
