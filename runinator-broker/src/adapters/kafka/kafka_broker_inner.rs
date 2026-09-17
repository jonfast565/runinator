#[allow(unused_imports)]
use super::*;

#[cfg(feature = "kafka")]
pub(super) struct KafkaBrokerInner {
    pub(super) producer: rdkafka::producer::FutureProducer,
    pub(super) control_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) agent_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) effect_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) infrastructure_effect_consumers:
        Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) effect_result_consumers:
        Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) wake_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) ingress_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) event_consumers: Mutex<HashMap<String, Arc<rdkafka::consumer::StreamConsumer>>>,
    pub(super) pending: Mutex<HashMap<Uuid, PendingDelivery>>,
}

#[cfg(feature = "kafka")]
impl KafkaBrokerInner {
    pub(super) fn new(config: &KafkaBrokerConfig) -> Result<Self, BrokerError> {
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

    pub(super) fn consumer(
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

    pub(super) fn track_delivery(
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

    pub(super) fn take_pending(&self, delivery_id: Uuid) -> Result<PendingDelivery, BrokerError> {
        self.pending
            .lock()
            .remove(&delivery_id)
            .ok_or(BrokerError::UnknownDelivery(delivery_id))
    }
}
