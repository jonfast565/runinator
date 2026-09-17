#[allow(unused_imports)]
use super::*;

pub(super) struct InstrumentedBroker {
    pub(super) inner: Arc<dyn Broker>,
    pub(super) metrics: BrokerMetrics,
}

#[async_trait]
impl Broker for InstrumentedBroker {
    fn supports_workflow_effect_channels(&self) -> bool {
        self.inner.supports_workflow_effect_channels()
    }

    fn supports_agent_channel(&self) -> bool {
        self.inner.supports_agent_channel()
    }

    fn connection_state(&self) -> Option<tokio::sync::watch::Receiver<ConnectionState>> {
        self.inner.connection_state()
    }

    async fn heartbeat(&self) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.heartbeat().await;
        self.metrics
            .record(CH_CONNECTION, "heartbeat", start, &result, true);
        result
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_control(command).await;
        self.metrics
            .record(CH_CONTROL, "publish", start, &result, true);
        result
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_control(consumer).await;
        self.metrics
            .record(CH_CONTROL, "receive", start, &result, false);
        result
    }

    async fn receive_control_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<ControlDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_control_for(profile).await;
        self.metrics
            .record(CH_CONTROL, "receive", start, &result, false);
        result
    }

    async fn ack_control(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_control(consumer, delivery_id).await;
        self.metrics.record(CH_CONTROL, "ack", start, &result, true);
        result
    }

    async fn nack_control(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_control(consumer, delivery_id).await;
        self.metrics
            .record(CH_CONTROL, "nack", start, &result, true);
        result
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_agent(command).await;
        self.metrics
            .record(CH_AGENT, "publish", start, &result, true);
        result
    }

    async fn receive_agent(&self, consumer: &str) -> Result<AgentDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_agent(consumer).await;
        self.metrics
            .record(CH_AGENT, "receive", start, &result, false);
        result
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_agent_for(profile).await;
        self.metrics
            .record(CH_AGENT, "receive", start, &result, false);
        result
    }

    async fn ack_agent(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_agent(consumer, delivery_id).await;
        self.metrics.record(CH_AGENT, "ack", start, &result, true);
        result
    }

    async fn nack_agent(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_agent(consumer, delivery_id).await;
        self.metrics.record(CH_AGENT, "nack", start, &result, true);
        result
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_effect(message).await;
        self.metrics
            .record(CH_EFFECT, "publish", start, &result, true);
        result
    }

    async fn receive_effect(&self, consumer: &str) -> Result<EffectDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_effect(consumer).await;
        self.metrics
            .record(CH_EFFECT, "receive", start, &result, false);
        result
    }

    async fn receive_effect_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<EffectDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_effect_for(profile).await;
        self.metrics
            .record(CH_EFFECT, "receive", start, &result, false);
        result
    }

    async fn receive_infrastructure_effect(
        &self,
        consumer: &str,
    ) -> Result<EffectDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_infrastructure_effect(consumer).await;
        self.metrics
            .record(CH_EFFECT, "receive", start, &result, false);
        result
    }

    async fn ack_effect(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_effect(consumer, delivery_id).await;
        self.metrics.record(CH_EFFECT, "ack", start, &result, true);
        result
    }

    async fn nack_effect(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_effect(consumer, delivery_id).await;
        self.metrics.record(CH_EFFECT, "nack", start, &result, true);
        result
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_effect_result(message).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "publish", start, &result, true);
        result
    }

    async fn receive_effect_result(
        &self,
        consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_effect_result(consumer).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "receive", start, &result, false);
        result
    }

    async fn ack_effect_result(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_effect_result(consumer, delivery_id).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "ack", start, &result, true);
        result
    }

    async fn nack_effect_result(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_effect_result(consumer, delivery_id).await;
        self.metrics
            .record(CH_EFFECT_RESULT, "nack", start, &result, true);
        result
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_wake(message).await;
        self.metrics
            .record(CH_WAKE, "publish", start, &result, true);
        result
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_wake(consumer).await;
        self.metrics
            .record(CH_WAKE, "receive", start, &result, false);
        result
    }

    async fn ack_wake(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_wake(consumer, delivery_id).await;
        self.metrics.record(CH_WAKE, "ack", start, &result, true);
        result
    }

    async fn nack_wake(&self, consumer: &str, delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_wake(consumer, delivery_id).await;
        self.metrics.record(CH_WAKE, "nack", start, &result, true);
        result
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_ingress(message).await;
        self.metrics
            .record(CH_INGRESS, "publish", start, &result, true);
        result
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_ingress(consumer).await;
        self.metrics
            .record(CH_INGRESS, "receive", start, &result, false);
        result
    }

    async fn ack_ingress(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.ack_ingress(consumer, delivery_id).await;
        self.metrics.record(CH_INGRESS, "ack", start, &result, true);
        result
    }

    async fn nack_ingress(
        &self,
        consumer: &str,
        delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.nack_ingress(consumer, delivery_id).await;
        self.metrics
            .record(CH_INGRESS, "nack", start, &result, true);
        result
    }

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        let start = Instant::now();
        let result = self.inner.publish_event(message).await;
        self.metrics
            .record(CH_EVENT, "publish", start, &result, true);
        result
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        let start = Instant::now();
        let result = self.inner.receive_event(consumer).await;
        self.metrics
            .record(CH_EVENT, "receive", start, &result, false);
        result
    }
}
