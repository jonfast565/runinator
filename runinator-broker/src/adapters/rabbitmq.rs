use crate::{Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery};
use async_trait::async_trait;

pub struct RabbitMqBroker;

impl RabbitMqBroker {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Broker for RabbitMqBroker {
    async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq publish"))
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq receive"))
    }

    async fn ack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq ack"))
    }

    async fn nack(&self, _consumer: &str, _delivery_id: uuid::Uuid) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq nack"))
    }

    async fn publish_control(&self, _message: ControlCommand) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq publish_control"))
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq receive_control"))
    }

    async fn ack_control(
        &self,
        _consumer: &str,
        _delivery_id: uuid::Uuid,
    ) -> Result<(), BrokerError> {
        Err(BrokerError::NotImplemented("rabbitmq ack_control"))
    }
}
