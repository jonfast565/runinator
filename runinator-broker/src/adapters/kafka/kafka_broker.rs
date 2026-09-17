#[allow(unused_imports)]
use super::*;

pub struct KafkaBroker {
    pub(super) config: KafkaBrokerConfig,
    #[cfg(feature = "kafka")]
    pub(super) inner: KafkaBrokerInner,
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
impl KafkaBroker {
    pub(super) async fn receive_effect_from(
        &self,
        channel: KafkaChannel,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        loop {
            let (message, pending) = receive_json::<EffectMessage>(self, channel, consumer).await?;
            if message.is_expired_at(chrono::Utc::now()) {
                ack_pending(PendingDelivery {
                    consumer: pending.consumer,
                    topic: pending.topic,
                    partition: pending.partition,
                    offset: pending.offset,
                })?;
                continue;
            }
            let delivery = EffectDelivery::from(message);
            self.inner.track_delivery(
                delivery.delivery_id,
                pending.consumer,
                pending.topic,
                pending.partition,
                pending.offset,
            );
            return Ok(delivery);
        }
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
