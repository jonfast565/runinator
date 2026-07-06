#[cfg(feature = "rabbitmq")]
use crate::{ActionTarget, ConsumerProfile};
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
    // second action queue carrying only `Labels`/`Replica`-targeted actions, so `Any` traffic (the
    // common case) never shares a queue with targeted traffic. See `Broker::receive_for`'s default
    // safety net: RabbitMQ can't natively express "message selector ⊆ consumer labels" for open-ended
    // labels, so this queue is deliberately coarse and every delivery from it is re-validated
    // client-side against the requesting profile before being handed back.
    pub targeted_action_queue: String,
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
            targeted_action_queue: format!("{DEFAULT_ACTION_QUEUE}.targeted"),
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

    /// override the queue that carries `Labels`/`Replica`-targeted actions (see field doc comment).
    pub fn with_targeted_action_queue(mut self, targeted_action_queue: impl Into<String>) -> Self {
        self.targeted_action_queue = targeted_action_queue.into();
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
use log::{info, warn};
#[cfg(feature = "rabbitmq")]
use parking_lot::Mutex;
#[cfg(feature = "rabbitmq")]
use std::{collections::HashMap, sync::Arc};
#[cfg(feature = "rabbitmq")]
use tokio::sync::Mutex as AsyncMutex;

#[cfg(feature = "rabbitmq")]
struct RabbitMqBrokerInner {
    // wrapped in AsyncMutex so ensure_connected can replace it after a connection drop.
    channel: AsyncMutex<lapin::Channel>,
    uri: String,
    action_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    targeted_action_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
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
    TargetedAction,
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
        declare_queue(&channel, &config.targeted_action_queue).await?;
        declare_queue(&channel, &config.control_queue).await?;
        declare_queue(&channel, &config.result_queue).await?;
        declare_queue(&channel, &config.wake_queue).await?;
        declare_queue(&channel, &config.ingress_queue).await?;
        declare_fanout_exchange(&channel, &config.event_exchange).await?;

        Ok(Self {
            channel: AsyncMutex::new(channel),
            uri: config.uri.clone(),
            action_consumers: Mutex::new(HashMap::new()),
            targeted_action_consumers: Mutex::new(HashMap::new()),
            control_consumers: Mutex::new(HashMap::new()),
            result_consumers: Mutex::new(HashMap::new()),
            wake_consumers: Mutex::new(HashMap::new()),
            ingress_consumers: Mutex::new(HashMap::new()),
            event_consumers: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        })
    }

    /// return a connected channel, reconnecting (and re-declaring queues/exchanges) if the current
    /// channel has closed. callers must release the returned clone before re-entering this method.
    async fn ensure_connected(
        &self,
        config: &RabbitMqBrokerConfig,
    ) -> Result<lapin::Channel, BrokerError> {
        use lapin::{Connection, ConnectionProperties};

        let mut guard = self.channel.lock().await;
        if guard.status().connected() {
            return Ok(guard.clone());
        }
        warn!("rabbitmq channel closed, attempting to reconnect");
        let conn = Connection::connect(&self.uri, ConnectionProperties::default())
            .await
            .map_err(rabbitmq_error("reconnect"))?;
        let new_channel = conn
            .create_channel()
            .await
            .map_err(rabbitmq_error("reconnect_channel"))?;
        declare_queue(&new_channel, &config.action_queue).await?;
        declare_queue(&new_channel, &config.targeted_action_queue).await?;
        declare_queue(&new_channel, &config.control_queue).await?;
        declare_queue(&new_channel, &config.result_queue).await?;
        declare_queue(&new_channel, &config.wake_queue).await?;
        declare_queue(&new_channel, &config.ingress_queue).await?;
        declare_fanout_exchange(&new_channel, &config.event_exchange).await?;
        // consumers are bound to the old channel; clear them so they're recreated on next use.
        self.action_consumers.lock().clear();
        self.targeted_action_consumers.lock().clear();
        self.control_consumers.lock().clear();
        self.result_consumers.lock().clear();
        self.wake_consumers.lock().clear();
        self.ingress_consumers.lock().clear();
        self.event_consumers.lock().clear();
        *guard = new_channel.clone();
        info!("reconnected to rabbitmq");
        Ok(new_channel)
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

        let ch = self.ensure_connected(config).await?;
        // a per-subscriber exclusive, auto-delete queue bound to the fanout exchange gives this
        // replica its own copy of every event; auto-ack since UI events are best-effort.
        let queue_name = format!("{}.events.{}", config.client_id, consumer_id);
        ch.queue_declare(
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
        ch.queue_bind(
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
            ch.basic_consume(
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
            RabbitMqChannel::TargetedAction => &self.targeted_action_consumers,
            RabbitMqChannel::Control => &self.control_consumers,
            RabbitMqChannel::Result => &self.result_consumers,
            RabbitMqChannel::Wake => &self.wake_consumers,
            RabbitMqChannel::Ingress => &self.ingress_consumers,
        };

        if let Some(consumer) = map.lock().get(consumer_id).cloned() {
            return Ok(consumer);
        }

        let ch = self.ensure_connected(config).await?;
        let queue = queue_for(config, channel);
        let tag = format!(
            "{}.{}.{}",
            config.client_id,
            channel_name(channel),
            consumer_id
        );
        let consumer = Arc::new(AsyncMutex::new(
            ch.basic_consume(
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
        RabbitMqChannel::Action => &config.action_queue,
        RabbitMqChannel::TargetedAction => &config.targeted_action_queue,
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
        RabbitMqChannel::TargetedAction => "actions.targeted",
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

#[cfg(feature = "rabbitmq")]
impl RabbitMqBroker {
    /// receive the next delivery from the targeted-actions queue (`Labels`/`Replica` targets only).
    /// unlike `receive`, callers must re-validate the delivery's target against their own profile —
    /// this queue can carry cross-talk for other consumers' label groups.
    async fn receive_targeted_action(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        let result =
            receive_json::<BrokerMessage>(self, RabbitMqChannel::TargetedAction, consumer).await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner.targeted_action_consumers.lock().remove(consumer);
        }
        let (message, delivery) = result?;
        let broker_delivery = BrokerDelivery::from(message);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }
}

#[async_trait]
#[cfg(feature = "rabbitmq")]
impl Broker for RabbitMqBroker {
    fn supports_workflow_result_channels(&self) -> bool {
        self.config.has_workflow_result_queue()
    }

    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        // `Any` traffic (the common case) keeps using the plain shared queue unchanged; `Labels`/
        // `Replica` targets go to the second queue so general workers never see them at all. See
        // `receive_for`'s override below for how a targeted delivery is matched to the right consumer.
        let queue = match &message.command.target {
            ActionTarget::Any => &self.config.action_queue,
            ActionTarget::Labels { .. } | ActionTarget::Replica { .. } => {
                &self.config.targeted_action_queue
            }
        };
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        let ch = self.inner.ensure_connected(&self.config).await?;
        publish_json(&ch, queue, &key, payload).await
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        let result = receive_json::<BrokerMessage>(self, RabbitMqChannel::Action, consumer).await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner.action_consumers.lock().remove(consumer);
        }
        let (message, delivery) = result?;
        let broker_delivery = BrokerDelivery::from(message);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    /// non-exclusive profiles (ordinary cloud workers, including non-exclusive-but-labeled
    /// org-dedicated workers) can legitimately receive both `Any` work and any `Labels`/`Replica`
    /// target they satisfy, so race both queues. exclusive profiles (e.g. the desktop worker) never
    /// match `Any` (see `ActionTarget::matches`), so only the targeted queue is worth draining.
    /// either way, since RabbitMQ can't natively filter the targeted queue by content, every
    /// delivery pulled from it is re-validated against `profile` and requeued if it doesn't match.
    async fn receive_for(&self, profile: &ConsumerProfile) -> Result<BrokerDelivery, BrokerError> {
        let targeted = async {
            loop {
                let delivery = self.receive_targeted_action(&profile.id).await?;
                if delivery.command.target.matches(profile) {
                    return Ok(delivery);
                }
                self.nack(&profile.id, delivery.delivery_id).await?;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        };
        if profile.exclusive {
            targeted.await
        } else {
            tokio::select! {
                result = self.receive(&profile.id) => result,
                result = targeted => result,
            }
        }
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
        let ch = self.inner.ensure_connected(&self.config).await?;
        publish_json(&ch, &self.config.control_queue, &key, payload).await
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        let result = receive_json::<ControlCommand>(self, RabbitMqChannel::Control, consumer).await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner.control_consumers.lock().remove(consumer);
        }
        let (command, delivery) = result?;
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
        let ch = self.inner.ensure_connected(&self.config).await?;
        publish_json(&ch, &self.config.result_queue, &key, payload).await
    }

    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
        let result = receive_json::<ResultMessage>(self, RabbitMqChannel::Result, consumer).await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner.result_consumers.lock().remove(consumer);
        }
        let (message, delivery) = result?;
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
        let ch = self.inner.ensure_connected(&self.config).await?;
        publish_json(&ch, &self.config.wake_queue, &key, payload).await
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        let result = receive_json::<WakeMessage>(self, RabbitMqChannel::Wake, consumer).await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner.wake_consumers.lock().remove(consumer);
        }
        let (message, delivery) = result?;
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
        let ch = self.inner.ensure_connected(&self.config).await?;
        publish_json(&ch, &self.config.ingress_queue, &key, payload).await
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        let result = receive_json::<IngressMessage>(self, RabbitMqChannel::Ingress, consumer).await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner.ingress_consumers.lock().remove(consumer);
        }
        let (message, delivery) = result?;
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
        let ch = self.inner.ensure_connected(&self.config).await?;
        publish_fanout(&ch, &self.config.event_exchange, payload).await
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        let subscriber = self.inner.event_consumer(&self.config, consumer).await?;
        let delivery_result = {
            let mut guard = subscriber.lock().await;
            guard.next().await
        };
        let Some(delivery) = delivery_result else {
            self.inner.event_consumers.lock().remove(consumer);
            return Err(BrokerError::ConsumerStreamEnded);
        };
        let delivery = delivery.map_err(rabbitmq_error("receive_event"))?;
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
    fn rabbitmq_config_defaults_targeted_action_queue() {
        let config = RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f");

        assert_eq!(
            config.targeted_action_queue,
            format!("{DEFAULT_ACTION_QUEUE}.targeted")
        );

        let config = config.with_targeted_action_queue("custom.targeted");
        assert_eq!(config.targeted_action_queue, "custom.targeted");
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
