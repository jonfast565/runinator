#[allow(unused_imports)]
use super::*;

pub struct RabbitMqBroker {
    pub(super) config: RabbitMqBrokerConfig,
    #[cfg(feature = "rabbitmq")]
    pub(super) inner: RabbitMqBrokerInner,
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
impl RabbitMqBroker {
    pub(super) async fn receive_effect_from(
        &self,
        channel: RabbitMqChannel,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        loop {
            let result = receive_json::<EffectMessage>(self, channel, consumer).await;
            if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
                match channel {
                    RabbitMqChannel::Effect => {
                        self.inner.effect_consumers.lock().remove(consumer);
                    }
                    RabbitMqChannel::InfrastructureEffect => {
                        self.inner
                            .infrastructure_effect_consumers
                            .lock()
                            .remove(consumer);
                    }
                    _ => {}
                }
            }
            let (message, delivery) = result?;
            if message.is_expired_at(chrono::Utc::now()) {
                ack_delivery(delivery).await?;
                continue;
            }
            let broker_delivery = EffectDelivery::from(message);
            self.inner
                .track_delivery(broker_delivery.delivery_id, delivery);
            return Ok(broker_delivery);
        }
    }
}

#[async_trait]
#[cfg(feature = "rabbitmq")]
impl Broker for RabbitMqBroker {
    fn supports_agent_channel(&self) -> bool {
        !self.config.agent_queue_prefix.trim().is_empty()
    }

    fn supports_workflow_effect_channels(&self) -> bool {
        self.config.has_workflow_effect_queues()
    }

    async fn heartbeat(&self) -> Result<(), BrokerError> {
        // `ensure_connected` uses RabbitMQ's protocol-level connection heartbeat and recreates a
        // dropped channel before the next receive; no queue message is emitted for health.
        self.inner.ensure_connected(&self.config).await.map(|_| ())
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let key = command.workflow_run_id.to_string();
        let payload = serde_json::to_string(&command)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        let ch = self.inner.ensure_connected(&self.config).await?;
        RabbitChannel(&ch)
            .publish(&self.config.control_queue, &key, payload)
            .await
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

    async fn nack_control(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let queue = agent_queue(&self.config, command.replica_id);
        let payload = serde_json::to_string(&command)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        let ch = self.inner.ensure_connected(&self.config).await?;
        RabbitChannel(&ch).declare_queue(&queue).await?;
        RabbitChannel(&ch)
            .publish(&queue, &command.directive_id.to_string(), payload)
            .await
    }

    async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
        let replica_id = Uuid::parse_str(consumer)
            .map_err(|_| BrokerError::Internal("agent consumer must be a replica uuid".into()))?;
        self.receive_agent_for(&ConsumerProfile {
            id: consumer.to_string(),
            replica_id: Some(replica_id),
            labels: Default::default(),
            exclusive: true,
        })
        .await
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        let result = receive_agent_json(self, profile).await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner
                .agent_consumers
                .lock()
                .retain(|key, _| !key.ends_with(&format!(":{}", profile.id)));
        }
        let (command, delivery) = result?;
        let broker_delivery = AgentDelivery::from(command);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    async fn ack_agent(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn nack_agent(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let queue = match message.command.executor {
            EffectExecutor::Provider => &self.config.effect_queue,
            EffectExecutor::Infrastructure => &self.config.infrastructure_effect_queue,
        };
        let expires_at = message.effective_expires_at();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        let ch = self.inner.ensure_connected(&self.config).await?;
        match expires_at {
            Some(expires_at) => {
                RabbitChannel(&ch)
                    .publish_expiring(queue, &key, payload, expires_at)
                    .await
            }
            None => RabbitChannel(&ch).publish(queue, &key, payload).await,
        }
    }

    async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
        self.receive_effect_from(RabbitMqChannel::Effect, consumer)
            .await
    }

    async fn receive_infrastructure_effect(
        &self,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        self.receive_effect_from(RabbitMqChannel::InfrastructureEffect, consumer)
            .await
    }

    async fn ack_effect(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn nack_effect(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        let ch = self.inner.ensure_connected(&self.config).await?;
        RabbitChannel(&ch)
            .publish(&self.config.effect_result_queue, &key, payload)
            .await
    }

    async fn receive_effect_result(
        &self,
        consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        let result =
            receive_json::<EffectResultMessage>(self, RabbitMqChannel::EffectResult, consumer)
                .await;
        if matches!(result, Err(BrokerError::ConsumerStreamEnded)) {
            self.inner.effect_result_consumers.lock().remove(consumer);
        }
        let (message, delivery) = result?;
        let broker_delivery = EffectResultDelivery::from(message);
        self.inner
            .track_delivery(broker_delivery.delivery_id, delivery);
        Ok(broker_delivery)
    }

    async fn ack_effect_result(
        &self,
        _consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        ack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn nack_effect_result(
        &self,
        _consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        nack_delivery(self.inner.take_pending(delivery_id)?).await
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let key = message.dedupe_key_or_hash();
        let payload = serde_json::to_string(&message)
            .map_err(|err| BrokerError::Internal(err.to_string()))?;
        let ch = self.inner.ensure_connected(&self.config).await?;
        RabbitChannel(&ch)
            .publish(&self.config.wake_queue, &key, payload)
            .await
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
        RabbitChannel(&ch)
            .publish(&self.config.ingress_queue, &key, payload)
            .await
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
        RabbitChannel(&ch)
            .publish_fanout(&self.config.event_exchange, payload)
            .await
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
    async fn publish_control(&self, _command: ControlCommand) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn ack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Err(rabbitmq_feature_error())
    }

    async fn nack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
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
