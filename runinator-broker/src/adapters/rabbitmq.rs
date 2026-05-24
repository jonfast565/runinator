use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    ResultDelivery, ResultMessage,
};
use async_trait::async_trait;
use uuid::Uuid;

const DEFAULT_ACTION_QUEUE: &str = "runinator.actions";
const DEFAULT_CONTROL_QUEUE: &str = "runinator.control";
const DEFAULT_RESULT_QUEUE: &str = "runinator.results";
const DEFAULT_CLIENT_ID: &str = "runinator";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RabbitMqBrokerConfig {
    pub uri: String,
    pub action_queue: String,
    pub control_queue: String,
    pub result_queue: String,
    pub client_id: String,
}

impl RabbitMqBrokerConfig {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            action_queue: DEFAULT_ACTION_QUEUE.into(),
            control_queue: DEFAULT_CONTROL_QUEUE.into(),
            result_queue: DEFAULT_RESULT_QUEUE.into(),
            client_id: DEFAULT_CLIENT_ID.into(),
        }
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

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = client_id.into();
        self
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
    pending: Mutex<HashMap<Uuid, lapin::message::Delivery>>,
}

#[cfg(feature = "rabbitmq")]
#[derive(Clone, Copy)]
enum RabbitMqChannel {
    Action,
    Control,
    Result,
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

        Ok(Self {
            channel,
            action_consumers: Mutex::new(HashMap::new()),
            control_consumers: Mutex::new(HashMap::new()),
            result_consumers: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        })
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
    }
}

#[cfg(feature = "rabbitmq")]
fn channel_name(channel: RabbitMqChannel) -> &'static str {
    match channel {
        RabbitMqChannel::Action => "actions",
        RabbitMqChannel::Control => "control",
        RabbitMqChannel::Result => "results",
    }
}

#[cfg(feature = "rabbitmq")]
fn rabbitmq_error(context: &'static str) -> impl FnOnce(lapin::Error) -> BrokerError {
    move |err| BrokerError::Internal(format!("rabbitmq {context}: {err}"))
}

#[async_trait]
#[cfg(feature = "rabbitmq")]
impl Broker for RabbitMqBroker {
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
}

#[async_trait]
#[cfg(not(feature = "rabbitmq"))]
impl Broker for RabbitMqBroker {
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
}
