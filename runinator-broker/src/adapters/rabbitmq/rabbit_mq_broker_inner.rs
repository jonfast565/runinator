#[allow(unused_imports)]
use super::*;

#[cfg(feature = "rabbitmq")]
pub(super) struct RabbitMqBrokerInner {
    // wrapped in AsyncMutex so ensure_connected can replace it after a connection drop.
    pub(super) channel: AsyncMutex<lapin::Channel>,
    pub(super) uri: String,
    pub(super) control_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pub(super) agent_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pub(super) effect_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pub(super) infrastructure_effect_consumers:
        Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pub(super) effect_result_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pub(super) wake_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pub(super) ingress_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    // each subscriber gets its own exclusive auto-delete queue bound to the fan-out exchange.
    pub(super) event_consumers: Mutex<HashMap<String, Arc<AsyncMutex<lapin::Consumer>>>>,
    pub(super) pending: Mutex<HashMap<Uuid, lapin::message::Delivery>>,
}

#[cfg(feature = "rabbitmq")]
impl RabbitMqBrokerInner {
    pub(super) async fn connect(config: &RabbitMqBrokerConfig) -> Result<Self, BrokerError> {
        use lapin::{Connection, ConnectionProperties};

        let connection = Connection::connect(&config.uri, ConnectionProperties::default())
            .await
            .map_err(rabbitmq_error("connect"))?;
        let channel = connection
            .create_channel()
            .await
            .map_err(rabbitmq_error("channel"))?;
        RabbitChannel(&channel).apply_qos(config).await?;
        RabbitChannel(&channel)
            .declare_queue(&config.control_queue)
            .await?;
        RabbitChannel(&channel)
            .declare_queue(&config.effect_queue)
            .await?;
        RabbitChannel(&channel)
            .declare_queue(&config.infrastructure_effect_queue)
            .await?;
        RabbitChannel(&channel)
            .declare_queue(&config.effect_result_queue)
            .await?;
        RabbitChannel(&channel)
            .declare_queue(&config.wake_queue)
            .await?;
        RabbitChannel(&channel)
            .declare_queue(&config.ingress_queue)
            .await?;
        RabbitChannel(&channel)
            .declare_fanout_exchange(&config.event_exchange)
            .await?;

        Ok(Self {
            channel: AsyncMutex::new(channel),
            uri: config.uri.clone(),
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

    /// return a connected channel, reconnecting (and re-declaring queues/exchanges) if the current
    /// channel has closed. callers must release the returned clone before re-entering this method.
    pub(super) async fn ensure_connected(
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
        RabbitChannel(&new_channel).apply_qos(config).await?;
        RabbitChannel(&new_channel)
            .declare_queue(&config.control_queue)
            .await?;
        RabbitChannel(&new_channel)
            .declare_queue(&config.effect_queue)
            .await?;
        RabbitChannel(&new_channel)
            .declare_queue(&config.infrastructure_effect_queue)
            .await?;
        RabbitChannel(&new_channel)
            .declare_queue(&config.effect_result_queue)
            .await?;
        RabbitChannel(&new_channel)
            .declare_queue(&config.wake_queue)
            .await?;
        RabbitChannel(&new_channel)
            .declare_queue(&config.ingress_queue)
            .await?;
        RabbitChannel(&new_channel)
            .declare_fanout_exchange(&config.event_exchange)
            .await?;
        // consumers are bound to the old channel; clear them so they're recreated on next use.
        self.control_consumers.lock().clear();
        self.agent_consumers.lock().clear();
        self.effect_consumers.lock().clear();
        self.infrastructure_effect_consumers.lock().clear();
        self.effect_result_consumers.lock().clear();
        self.wake_consumers.lock().clear();
        self.ingress_consumers.lock().clear();
        self.event_consumers.lock().clear();
        *guard = new_channel.clone();
        info!("reconnected to rabbitmq");
        Ok(new_channel)
    }

    /// get-or-create one subscriber's exclusive queue bound to the fan-out events exchange.
    pub(super) async fn event_consumer(
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
        let tag = format!(
            "{}.events.{}.{}",
            config.client_id,
            consumer_id,
            Uuid::new_v4()
        );
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

    pub(super) async fn agent_consumer(
        &self,
        config: &RabbitMqBrokerConfig,
        replica_id: Uuid,
        consumer_id: &str,
    ) -> Result<Arc<AsyncMutex<lapin::Consumer>>, BrokerError> {
        let key = format!("{replica_id}:{consumer_id}");
        if let Some(consumer) = self.agent_consumers.lock().get(&key).cloned() {
            return Ok(consumer);
        }
        let ch = self.ensure_connected(config).await?;
        let queue = agent_queue(config, replica_id);
        RabbitChannel(&ch).declare_queue(&queue).await?;
        let tag = format!(
            "{}.agent.{}.{}",
            config.client_id,
            consumer_id,
            Uuid::new_v4()
        );
        let consumer = Arc::new(AsyncMutex::new(
            ch.basic_consume(
                queue.as_str().into(),
                tag.into(),
                lapin::options::BasicConsumeOptions::default(),
                lapin::types::FieldTable::default(),
            )
            .await
            .map_err(rabbitmq_error("agent_consume"))?,
        ));
        self.agent_consumers
            .lock()
            .insert(key, Arc::clone(&consumer));
        Ok(consumer)
    }

    pub(super) async fn consumer(
        &self,
        config: &RabbitMqBrokerConfig,
        channel: RabbitMqChannel,
        consumer_id: &str,
    ) -> Result<Arc<AsyncMutex<lapin::Consumer>>, BrokerError> {
        let map = match channel {
            RabbitMqChannel::Control => &self.control_consumers,
            RabbitMqChannel::Effect => &self.effect_consumers,
            RabbitMqChannel::InfrastructureEffect => &self.infrastructure_effect_consumers,
            RabbitMqChannel::EffectResult => &self.effect_result_consumers,
            RabbitMqChannel::Wake => &self.wake_consumers,
            RabbitMqChannel::Ingress => &self.ingress_consumers,
        };

        if let Some(consumer) = map.lock().get(consumer_id).cloned() {
            return Ok(consumer);
        }

        let ch = self.ensure_connected(config).await?;
        let queue = queue_for(config, channel);
        let tag = format!(
            "{}.{}.{}.{}",
            config.client_id,
            channel_name(channel),
            consumer_id,
            Uuid::new_v4()
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

    pub(super) fn track_delivery(&self, delivery_id: Uuid, delivery: lapin::message::Delivery) {
        self.pending.lock().insert(delivery_id, delivery);
    }

    pub(super) fn take_pending(
        &self,
        delivery_id: Uuid,
    ) -> Result<lapin::message::Delivery, BrokerError> {
        self.pending
            .lock()
            .remove(&delivery_id)
            .ok_or(BrokerError::UnknownDelivery(delivery_id))
    }
}
